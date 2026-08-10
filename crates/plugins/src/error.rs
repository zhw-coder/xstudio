use std::path::PathBuf;

/// 插件运行时错误。
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// 插件目录读取失败。
    #[error("插件目录读取失败: {0}")]
    ReadDirectory(#[from] std::io::Error),
    /// 动态库加载失败。
    #[error("插件动态库加载失败 {path}: {error}")]
    LoadLibrary {
        path: PathBuf,
        error: libloading::Error,
    },
    /// 插件入口缺失或调用失败。
    #[error("插件入口调用失败 {path}: {message}")]
    Entry { path: PathBuf, message: String },
    /// 插件 ABI 版本不匹配。
    #[error("插件 ABI 版本不匹配 {path}: expected {expected}, got {actual}")]
    AbiVersion {
        path: PathBuf,
        expected: u32,
        actual: u32,
    },
    /// 插件 JSON 协议错误。
    #[error("插件 JSON 协议错误 {path}: {message}")]
    Protocol { path: PathBuf, message: String },
    /// Env 工厂不存在。
    #[error("未找到可用的插件执行环境工厂")]
    EnvFactoryNotFound,
    /// 插件环境调用失败。
    #[error("插件执行环境调用失败: {0}")]
    EnvCall(String),
}

/// 插件运行时结果类型。
pub type PluginResult<T> = Result<T, PluginError>;
