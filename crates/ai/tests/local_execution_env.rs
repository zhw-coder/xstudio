//! 本地 ExecutionEnv 与路径解析集成测试。

use ai::agent::env::{
    ExecutionEnv, ExecutionEnvExecOptions, FileErrorCode, FindFilesOptions, LocalExecutionEnv,
    LocalExecutionEnvOptions, ReadBinaryRangeOptions, ReadTextRangeOptions,
};
use std::time::{SystemTime, UNIX_EPOCH};

/// 创建本测试独占的临时目录。
fn test_dir(name: &str) -> std::path::PathBuf {
    let timestamp =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock should be after Unix epoch").as_nanos();
    std::env::temp_dir().join(format!("xstudio-{name}-{}-{timestamp}", std::process::id()))
}

/// 验证相对路径及 `@` 前缀始终相对于环境工作目录解析。
#[test]
fn resolves_relative_and_at_prefixed_paths_from_cwd() {
    let cwd = std::env::temp_dir().join("xstudio-path-root");
    let env = LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: cwd.clone(), ..Default::default() })
        .expect("valid working directory should create environment");

    assert_eq!(env.resolve_path("nested/file.md"), cwd.join("nested").join("file.md").display().to_string());
    assert_eq!(env.resolve_path("@nested/file.md"), cwd.join("nested").join("file.md").display().to_string());
}

/// 验证空工作目录在构造执行环境时返回稳定错误。
#[test]
fn rejects_empty_working_directory() {
    let error = match LocalExecutionEnv::new(LocalExecutionEnvOptions::default()) {
        Err(error) => error,
        Ok(_) => panic!("empty working directory should be rejected"),
    };

    assert_eq!(error.code, FileErrorCode::Invalid);
    assert_eq!(error.message, "Working directory must not be empty");
}

/// 验证 Unix 命令达到超时时返回稳定的超时错误。
#[cfg(unix)]
#[tokio::test]
async fn terminates_command_process_group_after_timeout() {
    let env = LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: std::env::temp_dir(), ..Default::default() })
        .expect("valid working directory should create environment");

    let error = env
        .exec("sleep 10", Some(ExecutionEnvExecOptions { timeout: Some(0), ..Default::default() }))
        .await
        .expect_err("timed out command should fail");

    assert_eq!(error.code, FileErrorCode::Unknown);
    assert_eq!(error.message, "timeout:0");
}

/// 验证 Windows 环境可使用默认 PowerShell 执行命令。
#[cfg(windows)]
#[tokio::test]
async fn executes_windows_powershell_commands_with_default_shell() {
    let env = LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: std::env::temp_dir(), ..Default::default() })
        .expect("valid working directory should create environment");
    let result = env.exec("echo xstudio", None).await.expect("Windows default shell should execute commands");

    assert!(result.stdout.contains("xstudio"));
}

/// 验证文本范围读取仅返回请求范围，并提供下一页偏移量。
#[tokio::test]
async fn reads_text_range_without_loading_remaining_content() {
    let cwd = test_dir("text-range");
    tokio::fs::create_dir_all(&cwd).await.expect("should create test directory");
    tokio::fs::write(cwd.join("sample.txt"), "one\ntwo\nthree\nfour\n").await.expect("should write text fixture");
    let env = LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: cwd.clone(), ..Default::default() })
        .expect("valid working directory should create environment");

    let result = env
        .read_text_range(
            "sample.txt",
            ReadTextRangeOptions { offset: 2, limit: Some(2), max_lines: 10, max_bytes: 1024 },
        )
        .await
        .expect("should read text range");

    assert_eq!(result.content, "two\nthree");
    assert_eq!(result.line_count, 2);
    assert_eq!(result.next_offset, Some(4));
    tokio::fs::remove_dir_all(cwd).await.expect("should remove test directory");
}

/// 验证二进制范围读取仅分配并返回所需字节。
#[tokio::test]
async fn reads_binary_range_with_next_offset() {
    let cwd = test_dir("binary-range");
    tokio::fs::create_dir_all(&cwd).await.expect("should create test directory");
    tokio::fs::write(cwd.join("sample.bin"), [0_u8, 1, 2, 3, 4]).await.expect("should write binary fixture");
    let env = LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: cwd.clone(), ..Default::default() })
        .expect("valid working directory should create environment");

    let result = env
        .read_binary_range("sample.bin", ReadBinaryRangeOptions { offset: 1, limit: 3 })
        .await
        .expect("should read binary range");

    assert_eq!(result.content, vec![1, 2, 3]);
    assert_eq!(result.next_offset, Some(4));
    tokio::fs::remove_dir_all(cwd).await.expect("should remove test directory");
}

/// 验证文件查找达到上限后报告早停。
#[tokio::test]
async fn finds_files_with_limit_and_early_stop() {
    let cwd = test_dir("find-limit");
    tokio::fs::create_dir_all(&cwd).await.expect("should create test directory");
    tokio::fs::write(cwd.join("first.rs"), "").await.expect("should write first fixture");
    tokio::fs::write(cwd.join("second.rs"), "").await.expect("should write second fixture");
    let env = LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: cwd.clone(), ..Default::default() })
        .expect("valid working directory should create environment");

    let result = env
        .find_files(".", FindFilesOptions { glob: Some("*.rs".to_string()), limit: Some(1) })
        .await
        .expect("should find files");

    assert_eq!(result.files.len(), 1);
    assert!(result.limit_reached);
    tokio::fs::remove_dir_all(cwd).await.expect("should remove test directory");
}
