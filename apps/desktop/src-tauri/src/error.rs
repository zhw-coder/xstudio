use thiserror::Error;

/// 桌面端后端统一结果类型。
pub type AppResult<T> = Result<T, AppError>;

/// 桌面端后端统一错误类型。
#[derive(Debug, Error)]
pub enum AppError {
    /// 数据库操作失败。
    #[error("数据库操作失败: {0}")]
    Database(#[from] ormlite::Error),

    /// SQLite 底层操作失败。
    #[error("SQLite 底层操作失败: {0}")]
    Sqlx(#[from] ormlite::SqlxError),

    /// JSON 编解码失败。
    #[error("JSON 编解码失败: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML 编解码失败。
    #[error("YAML 编解码失败: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// 文件系统操作失败。
    #[error("文件系统操作失败: {0}")]
    Io(#[from] std::io::Error),

    /// 打开系统文件管理器任务失败。
    #[error("打开系统文件管理器失败: {0}")]
    OpenPath(String),

    /// Tauri 路径解析失败。
    #[error("Tauri 路径解析失败: {0}")]
    TauriPath(#[from] tauri::Error),

    /// Provider 不存在。
    #[error("Provider 不存在: {0}")]
    ProviderNotFound(String),

    /// API Provider 未注册。
    #[error("API Provider 未注册: {0}")]
    ApiProviderNotFound(String),

    /// AI Provider 调用失败。
    #[error("AI Provider 调用失败: {0}")]
    AiProvider(String),

    /// AI Harness 调用失败。
    #[error("AI Harness 调用失败: {0}")]
    AiHarness(String),

    /// 会话模型不存在。
    #[error("会话模型不存在: {record_key}")]
    SessionModelNotFound { record_key: String },

    /// 会话 AgentHarness 不存在。
    #[error("会话 AgentHarness 不存在: {0}")]
    ChatAgentHarnessNotFound(String),

    /// 当前项目不存在。
    #[error("当前项目不存在")]
    CurrentProjectNotFound,

    /// 搜索实体不存在。
    #[error("搜索实体不存在: {0}")]
    SearchEngineNotFound(String),

    /// 模板名无效。
    #[error("模板名无效: {0}")]
    InvalidTemplateName(String),

    /// 模板文件路径无效。
    #[error("模板文件路径无效: {0}")]
    InvalidTemplatePath(String),

    /// Skill 文件路径无效。
    #[error("Skill 文件路径无效: {0}")]
    InvalidSkillPath(String),

    /// 工具名称不存在。
    #[error("工具名称不存在: {0}")]
    ToolNotFound(String),

    /// 会话自动压缩阈值无效。
    #[error("会话自动压缩阈值必须在 1 到 100 之间: {0}")]
    InvalidCompactRatio(u8),

    /// 全局 Context 锁定失败。
    #[error("全局 Context 锁定失败: {0}")]
    ContextLock(String),
}

impl From<ai::model::StreamError> for AppError {
    /// 保留 AI Provider 错误上下文。
    fn from(error: ai::model::StreamError) -> Self {
        Self::AiProvider(error.to_string())
    }
}

impl From<ai::agent::harness::HarnessError> for AppError {
    /// 保留 AI Harness 错误上下文。
    fn from(error: ai::agent::harness::HarnessError) -> Self {
        Self::AiHarness(error.to_string())
    }
}

impl From<ai::agent::harness::AgentHarnessError> for AppError {
    /// 保留 AgentHarness 错误上下文。
    fn from(error: ai::agent::harness::AgentHarnessError) -> Self {
        Self::AiHarness(error.to_string())
    }
}

/// 将后端错误转为 Tauri command 可返回的字符串，并打印完整 Debug 信息。
pub fn command_error(error: AppError) -> String {
    eprintln!("桌面端后端命令失败: {error:?}");
    error.to_string()
}
