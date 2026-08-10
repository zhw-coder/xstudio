//! 具体 harness 文件会按 TypeScript 目录继续 1:1 补齐。

pub mod agent_harness;
pub mod compaction;
pub mod messages;
pub mod prompt_templates;
pub mod session;
pub mod skills;
pub mod types;

pub use compaction::{branch_summarization, utils as compaction_utils};
pub use messages::*;
pub use prompt_templates::*;
pub use session::*;
pub use skills::*;
pub use types::*;

/// Harness 结果类型别名。
pub type HarnessResult<T> = Result<T, HarnessError>;

/// Harness 通用错误。
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    /// 尚未实现的 harness 能力。
    #[error("Harness feature is not implemented yet: {0}")]
    NotImplemented(&'static str),
    /// 通用错误消息。
    #[error("{0}")]
    Message(String),
    /// IO 错误。
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// JSON 错误。
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

pub use agent_harness::*;
