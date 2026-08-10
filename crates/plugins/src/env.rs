use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use ai::agent::env::{
    ExecutionEnv, ExecutionEnvExecOptions, ExecutionResult, FileError, FileErrorCode, FileInfo,
    FindFilesOptions, FindFilesResult, LocalExecutionEnv, LocalExecutionEnvOptions,
    ReadBinaryRangeOptions, ReadBinaryRangeResult, ReadTextRangeOptions, ReadTextRangeResult,
};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::{error::PluginError, runtime::PluginHandle};

/// 使用宿主默认 LocalExecutionEnv 执行插件发起的 JSON 请求。
///
/// @param request 含 `cwd`、`operation` 与 `arguments` 的 Env JSON 请求。
pub async fn call_default_env(request: Value) -> Result<Value, PluginError> {
    let cwd = request
        .get("cwd")
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::EnvCall("宿主 Env 请求缺少 cwd".to_string()))?;
    let operation = request
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::EnvCall("宿主 Env 请求缺少 operation".to_string()))?;
    let arguments = request.get("arguments").cloned().unwrap_or(Value::Null);
    let env = LocalExecutionEnvFactory.create(Path::new(cwd))?;
    let path = |field: &str| {
        arguments
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| PluginError::EnvCall(format!("宿主 Env 请求缺少 {field}")))
    };

    match operation {
        "exists" => env.exists(path("path")?).await.map(Value::from),
        "read_text_file" => env.read_text_file(path("path")?).await.map(Value::from),
        "write_file" => {
            let content = arguments
                .get("content")
                .ok_or_else(|| PluginError::EnvCall("宿主 Env 请求缺少 content".to_string()))?;
            let content: Vec<u8> = serde_json::from_value(content.clone())
                .map_err(|error| PluginError::EnvCall(error.to_string()))?;
            env.write_file(path("path")?, &content)
                .await
                .map(|()| Value::Null)
        }
        "list_dir" => env.list_dir(path("path")?).await.and_then(to_json_value),
        "real_path" => env.real_path(path("path")?).await.map(Value::from),
        "create_dir" => {
            let recursive = arguments
                .get("recursive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            env.create_dir(path("path")?, recursive)
                .await
                .map(|()| Value::Null)
        }
        "remove" => {
            let recursive = arguments
                .get("recursive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let force = arguments
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            env.remove(path("path")?, recursive, force)
                .await
                .map(|()| Value::Null)
        }
        _ => Err(FileError {
            code: FileErrorCode::Unknown,
            message: format!("宿主 Env operation 尚未实现: {operation}"),
            path: None,
        }),
    }
    .map_err(|error| PluginError::EnvCall(error.to_string()))
}

/// 将可序列化的 Env 返回值转换为 JSON。
///
/// @param value Env 操作返回值。
fn to_json_value<T: serde::Serialize>(value: T) -> Result<Value, FileError> {
    serde_json::to_value(value).map_err(|error| FileError {
        code: FileErrorCode::Unknown,
        message: error.to_string(),
        path: None,
    })
}

/// 创建执行环境的统一接口；工作目录始终由调用方显式提供。
pub trait ExecutionEnvFactory: Send + Sync {
    /// 使用指定工作目录创建执行环境。
    /// @param cwd 项目工作目录。
    fn create(&self, cwd: &Path) -> Result<Arc<dyn ExecutionEnv>, PluginError>;
}

/// 内置本地执行环境工厂。
pub struct LocalExecutionEnvFactory;

impl ExecutionEnvFactory for LocalExecutionEnvFactory {
    fn create(&self, cwd: &Path) -> Result<Arc<dyn ExecutionEnv>, PluginError> {
        let env = LocalExecutionEnv::new(LocalExecutionEnvOptions {
            cwd: cwd.to_path_buf(),
            shell_path: None,
            shell_env: HashMap::new(),
        })
        .map_err(|error| PluginError::EnvCall(error.to_string()))?;
        Ok(Arc::new(env))
    }
}

/// 通过插件 JSON 协议代理的执行环境。
pub struct PluginExecutionEnv {
    /// 运行时工作目录。
    cwd: String,
    /// 提供环境能力的已加载插件。
    plugin: Arc<PluginHandle>,
}

impl PluginExecutionEnv {
    /// 创建插件执行环境。
    /// @param plugin 已加载插件。
    /// @param cwd 项目工作目录。
    pub fn new(plugin: Arc<PluginHandle>, cwd: &Path) -> Self {
        Self {
            cwd: cwd.display().to_string(),
            plugin,
        }
    }

    /// 调用插件环境能力并反序列化响应。
    async fn call<T: DeserializeOwned>(
        &self,
        operation: &str,
        arguments: Value,
    ) -> Result<T, FileError> {
        let response = self
            .plugin
            .call(json!({"kind":"env","operation":operation,"cwd":self.cwd,"arguments":arguments}))
            .map_err(plugin_file_error)?;
        serde_json::from_value(response).map_err(|error| FileError {
            code: FileErrorCode::Unknown,
            message: format!("插件环境响应无效: {error}"),
            path: None,
        })
    }
}

/// 插件执行环境工厂。
pub struct PluginExecutionEnvFactory {
    /// 提供环境能力的插件。
    plugin: Arc<PluginHandle>,
}

impl PluginExecutionEnvFactory {
    /// 创建指定插件的执行环境工厂。
    pub fn new(plugin: Arc<PluginHandle>) -> Self {
        Self { plugin }
    }
}

impl ExecutionEnvFactory for PluginExecutionEnvFactory {
    fn create(&self, cwd: &Path) -> Result<Arc<dyn ExecutionEnv>, PluginError> {
        Ok(Arc::new(PluginExecutionEnv::new(
            Arc::clone(&self.plugin),
            cwd,
        )))
    }
}

/// 转换插件错误为执行环境统一错误。
fn plugin_file_error(error: PluginError) -> FileError {
    FileError {
        code: FileErrorCode::Unknown,
        message: error.to_string(),
        path: None,
    }
}

#[async_trait]
impl ExecutionEnv for PluginExecutionEnv {
    fn cwd(&self) -> &str {
        &self.cwd
    }
    fn platform(&self) -> &str {
        "plugin"
    }
    fn resolve_path(&self, path: &str) -> String {
        path.to_string()
    }
    fn join_path(&self, base: &str, child: &str) -> String {
        PathBuf::from(base).join(child).display().to_string()
    }
    fn dirname_path(&self, path: &str) -> String {
        Path::new(path)
            .parent()
            .unwrap_or_else(|| Path::new(path))
            .display()
            .to_string()
    }
    fn basename_path(&self, path: &str) -> String {
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .to_string()
    }
    fn relative_path(&self, root: &str, path: &str) -> String {
        Path::new(path)
            .strip_prefix(root)
            .unwrap_or_else(|_| Path::new(path))
            .display()
            .to_string()
    }
    async fn exec(
        &self,
        command: &str,
        options: Option<ExecutionEnvExecOptions>,
    ) -> Result<ExecutionResult, FileError> {
        self.call("exec", json!({"command":command,"options":options}))
            .await
    }
    async fn read_text_file(&self, path: &str) -> Result<String, FileError> {
        self.call("read_text_file", json!({"path":path})).await
    }
    async fn read_text_range(
        &self,
        path: &str,
        options: ReadTextRangeOptions,
    ) -> Result<ReadTextRangeResult, FileError> {
        self.call("read_text_range", json!({"path":path,"options":options}))
            .await
    }
    async fn read_binary_file(&self, path: &str) -> Result<Vec<u8>, FileError> {
        self.call("read_binary_file", json!({"path":path})).await
    }
    async fn read_binary_range(
        &self,
        path: &str,
        options: ReadBinaryRangeOptions,
    ) -> Result<ReadBinaryRangeResult, FileError> {
        self.call("read_binary_range", json!({"path":path,"options":options}))
            .await
    }
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), FileError> {
        self.call("write_file", json!({"path":path,"content":content}))
            .await
    }
    async fn file_info(&self, path: &str) -> Result<FileInfo, FileError> {
        self.call("file_info", json!({"path":path})).await
    }
    async fn list_dir(&self, path: &str) -> Result<Vec<FileInfo>, FileError> {
        self.call("list_dir", json!({"path":path})).await
    }
    async fn find_files(
        &self,
        path: &str,
        options: FindFilesOptions,
    ) -> Result<FindFilesResult, FileError> {
        self.call("find_files", json!({"path":path,"options":options}))
            .await
    }
    async fn real_path(&self, path: &str) -> Result<String, FileError> {
        self.call("real_path", json!({"path":path})).await
    }
    async fn exists(&self, path: &str) -> Result<bool, FileError> {
        self.call("exists", json!({"path":path})).await
    }
    async fn create_dir(&self, path: &str, recursive: bool) -> Result<(), FileError> {
        self.call("create_dir", json!({"path":path,"recursive":recursive}))
            .await
    }
    async fn remove(&self, path: &str, recursive: bool, force: bool) -> Result<(), FileError> {
        self.call(
            "remove",
            json!({"path":path,"recursive":recursive,"force":force}),
        )
        .await
    }
    async fn create_temp_dir(&self, prefix: Option<&str>) -> Result<String, FileError> {
        self.call("create_temp_dir", json!({"prefix":prefix})).await
    }
    async fn create_temp_file(
        &self,
        prefix: Option<&str>,
        suffix: Option<&str>,
    ) -> Result<String, FileError> {
        self.call("create_temp_file", json!({"prefix":prefix,"suffix":suffix}))
            .await
    }
    async fn create_point(&self) -> Result<String, FileError> {
        self.call("create_point", Value::Null).await
    }
    async fn get_point(&self) -> Result<Option<String>, FileError> {
        self.call("get_point", Value::Null).await
    }
    async fn reset_point(&self, commit_id: &str) -> Result<(), FileError> {
        self.call("reset_point", json!({"commitId":commit_id}))
            .await
    }
    async fn cleanup(&self) -> Result<(), FileError> {
        self.call("cleanup", Value::Null).await
    }
}
