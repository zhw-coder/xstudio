//! 本地 `ExecutionEnv` 默认实现。
//! 默认 `ExecutionEnv` 实现：基于 Tokio 进程与文件系统 API，把 `ExecutionEnv` 协议落地到
//! 本地文件系统与本地 shell 上，供 Harness 在桌面环境中执行 shell 命令、读写文件、维护临时目录。

use async_trait::async_trait;
use git2::{IndexAddOption, Repository, ResetType, Signature};
use ignore::WalkBuilder;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader},
    process::Command,
    time::{timeout, Duration},
};
use uuid::Uuid;

use crate::agent::env::types::{
    ExecutionEnv, ExecutionEnvExecOptions, ExecutionResult, FileError, FileErrorCode, FileInfo, FileKind,
    FindFilesOptions, FindFilesResult, ReadBinaryRangeOptions, ReadBinaryRangeResult, ReadTextRangeOptions,
    ReadTextRangeResult,
};

/// Windows 子进程创建标志：不创建可见控制台窗口。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 把文件元数据映射为 `FileKind`。
fn file_kind_from_metadata(metadata: &std::fs::Metadata) -> Result<FileKind, FileError> {
    let file_type = metadata.file_type();
    if file_type.is_file() {
        Ok(FileKind::File)
    } else if file_type.is_dir() {
        Ok(FileKind::Directory)
    } else if file_type.is_symlink() {
        Ok(FileKind::Symlink)
    } else {
        Err(FileError { code: FileErrorCode::Invalid, message: "Unsupported file type".to_string(), path: None })
    }
}

/// 根据绝对路径与元数据构造 `FileInfo`。
fn file_info_from_metadata(path: &Path, metadata: std::fs::Metadata) -> Result<FileInfo, FileError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_else(|| path.to_str().unwrap_or_default())
        .to_string();
    let mtime_ms = metadata.modified().ok().and_then(system_time_ms).unwrap_or_default();
    Ok(FileInfo {
        name,
        path: path.display().to_string(),
        kind: file_kind_from_metadata(&metadata)?,
        size: metadata.len(),
        mtime_ms,
    })
}

/// 把 SystemTime 转成毫秒时间戳。
fn system_time_ms(value: SystemTime) -> Option<f64> {
    value.duration_since(UNIX_EPOCH).ok().map(|duration| duration.as_secs_f64() * 1000.0)
}

/// 把底层 IO 错误规范化为 `FileError`。
fn to_file_error(error: io::Error, path: Option<&Path>) -> FileError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => FileErrorCode::NotFound,
        io::ErrorKind::PermissionDenied => FileErrorCode::PermissionDenied,
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => FileErrorCode::Invalid,
        io::ErrorKind::Unsupported => FileErrorCode::NotSupported,
        _ => match error.raw_os_error() {
            Some(20) => FileErrorCode::NotDirectory,
            Some(21) => FileErrorCode::IsDirectory,
            _ => FileErrorCode::Unknown,
        },
    };
    FileError { code, message: error.to_string(), path: path.map(|value| value.display().to_string()) }
}

/// 通过 `fs::metadata` 判断路径是否存在，所有失败一律视为不存在。
async fn path_exists(path: &Path) -> bool {
    fs::metadata(path).await.is_ok()
}

/// 在 PATH 中查找 bash 可执行文件。
#[cfg(not(windows))]
async fn find_bash_on_path() -> Option<String> {
    let output = Command::new("which").arg("bash").stdout(Stdio::piped()).stderr(Stdio::null()).output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_match = stdout.lines().next()?.trim().to_string();
    if first_match.is_empty() || !path_exists(Path::new(&first_match)).await {
        None
    } else {
        Some(first_match)
    }
}

/// 将 Git 错误转换为 ExecutionEnv 统一文件错误。
fn git_error(error: git2::Error, path: &Path) -> FileError {
    FileError {
        code: FileErrorCode::Unknown,
        message: error.to_string(),
        path: Some(path.display().to_string()),
    }
}

/// 检查点仓库中的固定 Git 作者名称。
const CHECKPOINT_AUTHOR_NAME: &str = "xstudio";
/// 检查点仓库中的固定 Git 作者邮箱。
const CHECKPOINT_AUTHOR_EMAIL: &str = "xstudio@local";
/// 检查点提交说明。
const CHECKPOINT_COMMIT_MESSAGE: &str = "xstudio checkpoint";
/// 检查点裸仓库相对目录。
const CHECKPOINT_REPOSITORY_PATH: &str = ".xstudio/.git";
/// 检查点仓库的内部排除规则。
const CHECKPOINT_EXCLUDE_RULES: [&str; 3] = [".xstudio/", "xstudio.sqlite", "db.sqlite"];

/// 打开检查点裸仓库并关联工作目录；首次调用完成仓库初始化。
fn open_checkpoint_repository(workdir: &Path) -> Result<(Repository, bool), FileError> {
    std::fs::create_dir_all(workdir).map_err(|error| to_file_error(error, Some(workdir)))?;
    let workdir = workdir.canonicalize().map_err(|error| to_file_error(error, Some(workdir)))?;
    let repository_path = workdir.join(CHECKPOINT_REPOSITORY_PATH);
    let is_new = !repository_path.exists();
    if is_new {
        let parent = repository_path.parent().ok_or_else(|| FileError {
            code: FileErrorCode::Invalid,
            message: "Checkpoint repository path has no parent directory".to_string(),
            path: Some(repository_path.display().to_string()),
        })?;
        std::fs::create_dir_all(parent).map_err(|error| to_file_error(error, Some(parent)))?;
    }
    let repository = if is_new {
        Repository::init_bare(&repository_path).map_err(|error| git_error(error, &repository_path))?
    } else {
        Repository::open_bare(&repository_path).map_err(|error| git_error(error, &repository_path))?
    };
    repository.set_workdir(&workdir, false).map_err(|error| git_error(error, &repository_path))?;
    Ok((repository, is_new))
}

/// 写入初始化检查点仓库所需的 Git 配置和内部排除规则。
fn initialize_checkpoint_repository(repository: &Repository) -> Result<(), FileError> {
    let mut config = repository.config().map_err(|error| git_error(error, repository.path()))?;
    config.set_str("user.name", CHECKPOINT_AUTHOR_NAME).map_err(|error| git_error(error, repository.path()))?;
    config.set_str("user.email", CHECKPOINT_AUTHOR_EMAIL).map_err(|error| git_error(error, repository.path()))?;

    let exclude_path = repository.path().join("info/exclude");
    let mut excludes = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    for rule in CHECKPOINT_EXCLUDE_RULES {
        if !excludes.lines().any(|line| line.trim() == rule) {
            if !excludes.is_empty() && !excludes.ends_with('\n') {
                excludes.push('\n');
            }
            excludes.push_str(rule);
            excludes.push('\n');
        }
    }
    std::fs::write(&exclude_path, excludes).map_err(|error| to_file_error(error, Some(&exclude_path)))
}

/// 将当前工作目录写入索引并返回对应树 ID。
fn checkpoint_tree(repository: &Repository) -> Result<git2::Oid, FileError> {
    let mut index = repository.index().map_err(|error| git_error(error, repository.path()))?;
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None).map_err(|error| git_error(error, repository.path()))?;
    index.write().map_err(|error| git_error(error, repository.path()))?;
    index.write_tree().map_err(|error| git_error(error, repository.path()))
}

/// 为指定树创建检查点提交。
fn commit_checkpoint_tree(repository: &Repository, tree_id: git2::Oid) -> Result<String, FileError> {
    let tree = repository.find_tree(tree_id).map_err(|error| git_error(error, repository.path()))?;
    let signature = Signature::now(CHECKPOINT_AUTHOR_NAME, CHECKPOINT_AUTHOR_EMAIL)
        .map_err(|error| git_error(error, repository.path()))?;
    let parent = repository.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents = parent.iter().collect::<Vec<_>>();
    repository
        .commit(Some("HEAD"), &signature, &signature, CHECKPOINT_COMMIT_MESSAGE, &tree, &parents)
        .map(|commit_id| commit_id.to_string())
        .map_err(|error| git_error(error, repository.path()))
}

/// 跨平台解析当前 `LocalExecutionEnv` 应使用的 shell。
async fn get_shell_config(custom_shell_path: Option<&str>) -> Result<(String, Vec<String>), FileError> {
    if let Some(custom_shell_path) = custom_shell_path {
        if path_exists(Path::new(custom_shell_path)).await {
            return Ok((custom_shell_path.to_string(), vec!["-c".to_string()]));
        }
        return Err(FileError {
            code: FileErrorCode::NotFound,
            message: format!("Custom shell path not found: {custom_shell_path}"),
            path: Some(custom_shell_path.to_string()),
        });
    }

    #[cfg(windows)]
    {
        return Ok((
            "powershell.exe".to_string(),
            vec!["-NoProfile".to_string(), "-NonInteractive".to_string(), "-Command".to_string()],
        ));
    }

    #[cfg(not(windows))]
    {
        if path_exists(Path::new("/bin/bash")).await {
            return Ok(("/bin/bash".to_string(), vec!["-c".to_string()]));
        }
        if let Some(bash) = find_bash_on_path().await {
            return Ok((bash, vec!["-c".to_string()]));
        }
        Ok(("sh".to_string(), vec!["-c".to_string()]))
    }
}

/// 在 Unix 上终止以 shell 为组长的整个命令进程组。
#[cfg(unix)]
async fn terminate_process_tree(pid: u32) {
    // 负 PID 表示目标进程组，shell 及其派生子进程在启动时被放入该组。
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}

/// 在 Windows 上通过 taskkill 终止 shell 及其全部子进程。
#[cfg(windows)]
async fn terminate_process_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await;
}

/// 按“当前进程环境 ← 构造时基底环境 ← 单次执行额外环境”的优先级顺序合并环境变量。
fn get_shell_env(base_env: &HashMap<String, String>, extra_env: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = std::env::vars().collect::<HashMap<_, _>>();
    env.extend(base_env.iter().map(|(key, value)| (key.to_string(), value.to_string())));
    env.extend(extra_env.iter().map(|(key, value)| (key.to_string(), value.to_string())));
    env
}

/// `ExecutionEnv` 协议的本地默认实现。
pub struct LocalExecutionEnv {
    /// Harness 当前会话使用的非空工作目录，可为相对或绝对路径。
    cwd: PathBuf,
    /// 显式指定的 shell 可执行文件路径。
    shell_path: Option<String>,
    /// 会话级基底环境变量。
    shell_env: HashMap<String, String>,
    /// 环境工作目录对应的唯一检查点裸仓库。
    checkpoint_repository: Mutex<Repository>,
}

impl LocalExecutionEnv {
    /// 构造一个本地执行环境。
    pub fn new(options: LocalExecutionEnvOptions) -> Result<Self, FileError> {
        if options.cwd.as_os_str().is_empty() {
            return Err(FileError {
                code: FileErrorCode::Invalid,
                message: "Working directory must not be empty".to_string(),
                path: None,
            });
        }
        let (checkpoint_repository, is_new) = open_checkpoint_repository(&options.cwd)?;
        if is_new {
            initialize_checkpoint_repository(&checkpoint_repository)?;
        }
        Ok(Self {
            cwd: options.cwd,
            shell_path: options.shell_path,
            shell_env: options.shell_env,
            checkpoint_repository: Mutex::new(checkpoint_repository),
        })
    }
}

/// `LocalExecutionEnv` 初始化参数。
#[derive(Clone, Debug, Default)]
pub struct LocalExecutionEnvOptions {
    /// 非空工作目录，可为相对或绝对路径。
    pub cwd: PathBuf,
    /// 自定义 shell 可执行文件路径。
    pub shell_path: Option<String>,
    /// 会话级基底环境变量。
    pub shell_env: HashMap<String, String>,
}

#[async_trait]
impl ExecutionEnv for LocalExecutionEnv {
    /// 当前工作目录。
    fn cwd(&self) -> &str {
        self.cwd.to_str().unwrap_or_default()
    }

    /// 当前本地执行环境所在的系统平台。
    fn platform(&self) -> &str {
        std::env::consts::OS
    }

    /// 将用户路径解析为环境内的绝对路径。
    fn resolve_path(&self, path: &str) -> String {
        let path = path.strip_prefix('@').unwrap_or(path);
        let expanded = if path == "~" {
            std::env::home_dir().unwrap_or_else(|| PathBuf::from(path))
        } else if let Some(rest) = path.strip_prefix("~/") {
            std::env::home_dir().map(|home| home.join(rest)).unwrap_or_else(|| PathBuf::from(path))
        } else if cfg!(windows) {
            path.strip_prefix("~\\")
                .and_then(|rest| std::env::home_dir().map(|home| home.join(rest)))
                .unwrap_or_else(|| PathBuf::from(path))
        } else {
            PathBuf::from(path)
        };
        if expanded.is_absolute() {
            expanded.display().to_string()
        } else {
            self.cwd.join(expanded).display().to_string()
        }
    }

    /// 拼接本地执行环境路径。
    fn join_path(&self, base: &str, child: &str) -> String {
        Path::new(base).join(child).display().to_string()
    }

    /// 取本地执行环境路径的父目录。
    fn dirname_path(&self, path: &str) -> String {
        Path::new(path).parent().unwrap_or_else(|| Path::new(path)).display().to_string()
    }

    /// 取本地执行环境路径的文件名。
    fn basename_path(&self, path: &str) -> String {
        Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path).to_string()
    }

    /// 计算本地执行环境路径相对 root 的路径。
    fn relative_path(&self, root: &str, path: &str) -> String {
        let path = Path::new(path);
        path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
    }

    /// 在当前执行环境的工作目录下执行 shell 命令。
    async fn exec(
        &self,
        command: &str,
        options: Option<ExecutionEnvExecOptions>,
    ) -> Result<ExecutionResult, FileError> {
        let options = options.unwrap_or_default();
        let cwd = options
            .cwd
            .as_deref()
            .map(|value| PathBuf::from(self.resolve_path(value)))
            .unwrap_or_else(|| self.cwd.clone());
        let (shell, args) = get_shell_config(self.shell_path.as_deref()).await?;
        let mut child = Command::new(shell);
        child
            .args(args)
            .arg(command)
            .current_dir(&cwd)
            .env_clear()
            .envs(get_shell_env(&self.shell_env, &options.env))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Unix 子进程以自身 PID 创建新进程组，超时时可一次清理整个命令树。
        #[cfg(unix)]
        child.process_group(0);
        // Windows PowerShell 不创建可见的控制台窗口。
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            child.creation_flags(CREATE_NO_WINDOW);
        }
        // 异步 future 被 timeout 取消时仍确保 shell 本身会被结束。
        child.kill_on_drop(true);
        let child = child.spawn().map_err(|error| to_file_error(error, Some(&cwd)))?;
        let pid = child.id().ok_or_else(|| FileError {
            code: FileErrorCode::Unknown,
            message: "Failed to determine shell process id".to_string(),
            path: Some(cwd.display().to_string()),
        })?;
        let future = child.wait_with_output();
        let output = if let Some(timeout_seconds) = options.timeout {
            match timeout(Duration::from_secs(timeout_seconds), future).await {
                Ok(result) => result.map_err(|error| to_file_error(error, Some(&cwd)))?,
                Err(_) => {
                    terminate_process_tree(pid).await;
                    return Err(FileError {
                        code: FileErrorCode::Unknown,
                        message: format!("timeout:{timeout_seconds}"),
                        path: None,
                    });
                }
            }
        } else {
            future.await.map_err(|error| to_file_error(error, Some(&cwd)))?
        };
        Ok(ExecutionResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or_default(),
        })
    }

    /// 以 UTF-8 读取文本文件内容。
    async fn read_text_file(&self, path: &str) -> Result<String, FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        fs::read_to_string(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))
    }

    /// 按行读取 UTF-8 文本范围，避免将整个文件载入内存。
    async fn read_text_range(
        &self,
        path: &str,
        options: ReadTextRangeOptions,
    ) -> Result<ReadTextRangeResult, FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        if options.offset == 0 || options.limit == Some(0) || options.max_lines == 0 || options.max_bytes == 0 {
            return Err(FileError {
                code: FileErrorCode::Invalid,
                message: "offset, limit, max_lines, and max_bytes must be positive".to_string(),
                path: Some(resolved.display().to_string()),
            });
        }
        let file = fs::File::open(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))?;
        let mut lines = BufReader::new(file).lines();
        let mut current_line = 0;
        while current_line < options.offset - 1 {
            if lines.next_line().await.map_err(|error| to_file_error(error, Some(&resolved)))?.is_none() {
                return Err(FileError {
                    code: FileErrorCode::Invalid,
                    message: format!("Offset {} is beyond end of file", options.offset),
                    path: Some(resolved.display().to_string()),
                });
            }
            current_line += 1;
        }

        let mut result = ReadTextRangeResult { start_line: options.offset, ..Default::default() };
        while let Some(line) = lines.next_line().await.map_err(|error| to_file_error(error, Some(&resolved)))? {
            current_line += 1;
            if options.limit.is_some_and(|limit| result.line_count >= limit) || result.line_count >= options.max_lines {
                result.next_offset = Some(current_line);
                return Ok(result);
            }
            let separator_bytes = usize::from(!result.content.is_empty());
            if result.content.len().saturating_add(separator_bytes).saturating_add(line.len()) > options.max_bytes {
                result.first_line_exceeds_limit = result.line_count == 0;
                result.next_offset = Some(current_line);
                return Ok(result);
            }
            if !result.content.is_empty() {
                result.content.push('\n');
            }
            result.content.push_str(&line);
            result.line_count += 1;
        }
        if result.line_count == 0 && options.offset > 1 {
            return Err(FileError {
                code: FileErrorCode::Invalid,
                message: format!("Offset {} is beyond end of file", options.offset),
                path: Some(resolved.display().to_string()),
            });
        }
        Ok(result)
    }

    /// 以二进制方式读取文件内容。
    async fn read_binary_file(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        fs::read(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))
    }

    /// 按字节读取二进制文件范围，避免将整个文件载入内存。
    async fn read_binary_range(
        &self,
        path: &str,
        options: ReadBinaryRangeOptions,
    ) -> Result<ReadBinaryRangeResult, FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        if options.limit == 0 {
            return Err(FileError {
                code: FileErrorCode::Invalid,
                message: "limit must be positive".to_string(),
                path: Some(resolved.display().to_string()),
            });
        }
        let mut file = fs::File::open(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))?;
        let file_size = file.metadata().await.map_err(|error| to_file_error(error, Some(&resolved)))?.len();
        if options.offset >= file_size {
            return Err(FileError {
                code: FileErrorCode::Invalid,
                message: format!("Offset {} is beyond end of file", options.offset),
                path: Some(resolved.display().to_string()),
            });
        }
        file.seek(std::io::SeekFrom::Start(options.offset))
            .await
            .map_err(|error| to_file_error(error, Some(&resolved)))?;
        let remaining = file_size - options.offset;
        let read_len = remaining.min(options.limit as u64) as usize;
        let mut content = vec![0; read_len];
        file.read_exact(&mut content).await.map_err(|error| to_file_error(error, Some(&resolved)))?;
        let next_offset = ((read_len as u64) < remaining).then_some(options.offset + read_len as u64);
        Ok(ReadBinaryRangeResult { content, next_offset })
    }

    /// 创建或覆盖文件并写入内容；若父目录不存在会自动创建。
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).await.map_err(|error| to_file_error(error, Some(parent)))?;
        }
        fs::write(&resolved, content).await.map_err(|error| to_file_error(error, Some(&resolved)))
    }

    /// 返回路径的元数据，使用 symlink metadata 不跟随 symlink。
    async fn file_info(&self, path: &str) -> Result<FileInfo, FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        let metadata = fs::symlink_metadata(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))?;
        file_info_from_metadata(&resolved, metadata)
    }

    /// 列出目录的直接子项。
    async fn list_dir(&self, path: &str) -> Result<Vec<FileInfo>, FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        let mut dir = fs::read_dir(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))?;
        let mut infos = Vec::new();
        while let Some(entry) = dir.next_entry().await.map_err(|error| to_file_error(error, Some(&resolved)))? {
            let entry_path = entry.path();
            let metadata =
                fs::symlink_metadata(&entry_path).await.map_err(|error| to_file_error(error, Some(&entry_path)))?;
            match file_info_from_metadata(&entry_path, metadata) {
                Ok(info) => infos.push(info),
                Err(error) if error.code == FileErrorCode::Invalid => {}
                Err(error) => return Err(error),
            }
        }
        Ok(infos)
    }

    /// 递归列出常规文件，并遵循 `.gitignore` 和 `.ignore` 规则。
    async fn find_files(&self, path: &str, options: FindFilesOptions) -> Result<FindFilesResult, FileError> {
        let root = PathBuf::from(self.resolve_path(path));
        fs::metadata(&root).await.map_err(|error| to_file_error(error, Some(&root)))?;
        let mut builder = WalkBuilder::new(&root);
        builder.hidden(false).git_ignore(true).parents(true).ignore(true);
        if let Some(pattern) = options.glob {
            let overrides = ignore::overrides::OverrideBuilder::new(&root)
                .add(&pattern)
                .map_err(|error| FileError {
                    code: FileErrorCode::Invalid,
                    message: format!("Invalid glob pattern: {error}"),
                    path: Some(root.display().to_string()),
                })?
                .build()
                .map_err(|error| FileError {
                    code: FileErrorCode::Invalid,
                    message: format!("Invalid glob pattern: {error}"),
                    path: Some(root.display().to_string()),
                })?;
            builder.overrides(overrides);
        }
        let mut result = FindFilesResult::default();
        for entry in builder.build().flatten() {
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if options.limit.is_some_and(|limit| result.files.len() >= limit) {
                result.limit_reached = true;
                break;
            }
            result.files.push(entry.into_path().display().to_string());
        }
        Ok(result)
    }

    /// 跟随 symlink 返回路径的规范化绝对形式。
    async fn real_path(&self, path: &str) -> Result<String, FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        fs::canonicalize(&resolved)
            .await
            .map(|value| value.display().to_string())
            .map_err(|error| to_file_error(error, Some(&resolved)))
    }

    /// 判断路径是否存在。
    async fn exists(&self, path: &str) -> Result<bool, FileError> {
        match self.file_info(path).await {
            Ok(_) => Ok(true),
            Err(error) if error.code == FileErrorCode::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// 创建目录，可选递归创建父目录。
    async fn create_dir(&self, path: &str, recursive: bool) -> Result<(), FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        if recursive {
            fs::create_dir_all(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))
        } else {
            fs::create_dir(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))
        }
    }

    /// 删除文件或目录。
    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), FileError> {
        let resolved = PathBuf::from(self.resolve_path(path));
        if force && !path_exists(&resolved).await {
            return Ok(());
        }
        let metadata = fs::symlink_metadata(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            if recursive {
                fs::remove_dir_all(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))
            } else {
                fs::remove_dir(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))
            }
        } else {
            fs::remove_file(&resolved).await.map_err(|error| to_file_error(error, Some(&resolved)))
        }
    }

    /// 在系统临时目录下创建一个全新的临时目录。
    async fn create_temp_dir(&self, prefix: Option<&str>) -> Result<String, FileError> {
        let prefix = prefix.unwrap_or("tmp-");
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!("{prefix}{}", Uuid::new_v4()));
            match fs::create_dir(&path).await {
                Ok(_) => return Ok(path.display().to_string()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(to_file_error(error, Some(&path))),
            }
        }
        Err(FileError {
            code: FileErrorCode::Unknown,
            message: "Failed to create temporary directory".to_string(),
            path: None,
        })
    }

    /// 在系统临时目录下创建一个空临时文件。
    async fn create_temp_file(&self, prefix: Option<&str>, suffix: Option<&str>) -> Result<String, FileError> {
        let dir = self.create_temp_dir(Some("tmp-")).await?;
        let path = Path::new(&dir).join(format!(
            "{}{}{}",
            prefix.unwrap_or_default(),
            Uuid::new_v4(),
            suffix.unwrap_or_default()
        ));
        fs::write(&path, b"").await.map_err(|error| to_file_error(error, Some(&path)))?;
        Ok(path.display().to_string())
    }

    /// 为环境工作目录创建检查点。
    async fn create_point(&self) -> Result<String, FileError> {
        let repository = self.checkpoint_repository.lock().map_err(|error| FileError {
            code: FileErrorCode::Unknown,
            message: format!("Checkpoint repository lock poisoned: {error}"),
            path: Some(self.cwd.display().to_string()),
        })?;
        let tree_id = checkpoint_tree(&repository)?;
        let head = repository.head().ok().and_then(|head| head.peel_to_commit().ok());
        if let Some(head) = head {
            if head.tree_id() == tree_id {
                return Ok(head.id().to_string());
            }
        }
        commit_checkpoint_tree(&repository, tree_id)
    }

    /// 返回环境工作目录当前检查点的提交 ID；未创建提交时返回空值。
    async fn get_point(&self) -> Result<Option<String>, FileError> {
        let repository = self.checkpoint_repository.lock().map_err(|error| FileError {
            code: FileErrorCode::Unknown,
            message: format!("Checkpoint repository lock poisoned: {error}"),
            path: Some(self.cwd.display().to_string()),
        })?;
        Ok(repository.head().ok().and_then(|head| head.peel_to_commit().ok()).map(|head| head.id().to_string()))
    }

    /// 强制将环境工作目录及索引回滚至指定检查点。
    async fn reset_point(&self, commit_id: &str) -> Result<(), FileError> {
        let repository = self.checkpoint_repository.lock().map_err(|error| FileError {
            code: FileErrorCode::Unknown,
            message: format!("Checkpoint repository lock poisoned: {error}"),
            path: Some(self.cwd.display().to_string()),
        })?;
        let target = repository.revparse_single(commit_id).map_err(|error| git_error(error, repository.path()))?;
        repository.reset(&target, ResetType::Hard, None).map_err(|error| git_error(error, repository.path()))
    }

    /// 释放执行环境持有的资源。
    async fn cleanup(&self) -> Result<(), FileError> {
        Ok(())
    }
}
