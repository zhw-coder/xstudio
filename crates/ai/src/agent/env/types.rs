//! `ExecutionEnv` 抽象协议及其文件系统、进程执行相关类型。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 文件系统对象类型。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// 普通文件。
    File,
    /// 目录。
    Directory,
    /// 符号链接。
    Symlink,
}

/// 文件操作稳定错误码。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileErrorCode {
    /// 路径不存在。
    NotFound,
    /// 权限不足。
    PermissionDenied,
    /// 不是目录。
    NotDirectory,
    /// 是目录。
    IsDirectory,
    /// 参数非法。
    Invalid,
    /// 后端不支持。
    NotSupported,
    /// 未知错误。
    Unknown,
}

/// 文件操作标准错误类型。
#[derive(Clone, Debug, thiserror::Error, Serialize, Deserialize, PartialEq, Eq)]
#[error("{message}")]
pub struct FileError {
    /// 稳定错误码。
    pub code: FileErrorCode,
    /// 人类可读描述。
    pub message: String,
    /// 失败路径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// 单个文件系统对象的元数据。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    /// basename。
    pub name: String,
    /// 相对于环境工作目录解析后的路径。
    pub path: String,
    /// 对象类型。
    pub kind: FileKind,
    /// 字节大小。
    pub size: u64,
    /// 修改时间毫秒。
    pub mtime_ms: f64,
}

/// `ExecutionEnv.exec` 的可选参数。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEnvExecOptions {
    /// 工作目录。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// 环境变量覆盖。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    /// 超时时间秒。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// `ExecutionEnv.exec` 的返回值。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    /// stdout。
    pub stdout: String,
    /// stderr。
    pub stderr: String,
    /// 退出码。
    pub exit_code: i32,
}

/// `ExecutionEnv.find_files` 的递归遍历选项。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FindFilesOptions {
    /// 可选 glob 过滤条件。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    /// 最多返回的文件数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// 递归文件查找结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FindFilesResult {
    /// 命中的普通文件路径，相对于环境工作目录解析。
    pub files: Vec<String>,
    /// 是否因达到结果上限而停止遍历。
    pub limit_reached: bool,
}

/// UTF-8 文本范围读取选项。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextRangeOptions {
    /// 开始读取的行号，从 1 开始。
    pub offset: usize,
    /// 最多读取的行数；为空时仅受输出上限约束。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// 最多返回的行数。
    pub max_lines: usize,
    /// 最多返回的 UTF-8 字节数。
    pub max_bytes: usize,
}

/// UTF-8 文本范围读取结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextRangeResult {
    /// 读取到的文本，不包含末尾换行。
    pub content: String,
    /// 输出的首行行号。
    pub start_line: usize,
    /// 输出行数。
    pub line_count: usize,
    /// 下一次读取应使用的行号；为空表示已到文件结尾。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,
    /// 首个目标行超过字节上限。
    pub first_line_exceeds_limit: bool,
}

/// 二进制范围读取选项。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadBinaryRangeOptions {
    /// 开始读取的字节偏移量，从 0 开始。
    pub offset: u64,
    /// 最多读取的字节数。
    pub limit: usize,
}

/// 二进制范围读取结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadBinaryRangeResult {
    /// 读取到的字节。
    pub content: Vec<u8>,
    /// 下一次读取应使用的字节偏移量；为空表示已到文件结尾。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u64>,
}

/// Agent 使用的文件系统与进程执行环境抽象。
#[async_trait]
pub trait ExecutionEnv: Send + Sync {
    /// 当前工作目录。
    fn cwd(&self) -> &str;
    /// 当前执行环境所在的系统平台。
    fn platform(&self) -> &str;
    /// 将用户路径解析为环境内路径；返回形式取决于环境工作目录是相对还是绝对路径。
    fn resolve_path(&self, path: &str) -> String;
    /// 拼接环境内路径。
    fn join_path(&self, base: &str, child: &str) -> String;
    /// 取环境内路径的父目录。
    fn dirname_path(&self, path: &str) -> String;
    /// 取环境内路径的文件名。
    fn basename_path(&self, path: &str) -> String;
    /// 计算环境内路径相对 root 的路径。
    fn relative_path(&self, root: &str, path: &str) -> String;
    /// 执行 shell 命令。
    async fn exec(&self, command: &str, options: Option<ExecutionEnvExecOptions>)
        -> Result<ExecutionResult, FileError>;
    /// 读取 UTF-8 文本文件。
    async fn read_text_file(&self, path: &str) -> Result<String, FileError>;
    /// 按行读取 UTF-8 文本文件范围，并在达到任一上限后停止。
    async fn read_text_range(
        &self,
        path: &str,
        options: ReadTextRangeOptions,
    ) -> Result<ReadTextRangeResult, FileError>;
    /// 读取二进制文件。
    async fn read_binary_file(&self, path: &str) -> Result<Vec<u8>, FileError>;
    /// 按字节范围读取二进制文件，并在达到上限后停止。
    async fn read_binary_range(
        &self,
        path: &str,
        options: ReadBinaryRangeOptions,
    ) -> Result<ReadBinaryRangeResult, FileError>;
    /// 创建或覆盖文件。
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), FileError>;
    /// 返回路径元数据。
    async fn file_info(&self, path: &str) -> Result<FileInfo, FileError>;
    /// 列出目录。
    async fn list_dir(&self, path: &str) -> Result<Vec<FileInfo>, FileError>;
    /// 递归列出常规文件，并遵循 `.gitignore` 和 `.ignore` 规则及结果上限。
    async fn find_files(&self, path: &str, options: FindFilesOptions) -> Result<FindFilesResult, FileError>;
    /// 返回真实路径。
    async fn real_path(&self, path: &str) -> Result<String, FileError>;
    /// 路径是否存在。
    async fn exists(&self, path: &str) -> Result<bool, FileError>;
    /// 创建目录。
    async fn create_dir(&self, path: &str, recursive: bool) -> Result<(), FileError>;
    /// 删除文件或目录。
    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), FileError>;
    /// 创建临时目录。
    async fn create_temp_dir(&self, prefix: Option<&str>) -> Result<String, FileError>;
    /// 创建临时文件。
    async fn create_temp_file(&self, prefix: Option<&str>, suffix: Option<&str>) -> Result<String, FileError>;
    /// 为环境工作目录创建检查点。
    async fn create_point(&self) -> Result<String, FileError>;
    /// 返回环境工作目录当前检查点的提交 ID；尚未创建检查点时返回空值。
    async fn get_point(&self) -> Result<Option<String>, FileError>;
    /// 将环境工作目录强制回滚到检查点提交。
    async fn reset_point(&self, commit_id: &str) -> Result<(), FileError>;
    /// 释放执行环境资源。
    async fn cleanup(&self) -> Result<(), FileError>;
}
