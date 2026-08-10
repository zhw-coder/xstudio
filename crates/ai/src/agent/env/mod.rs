//! Agent 执行环境抽象、实现及其辅助工具。

use std::sync::Arc;

pub mod local;
pub mod types;

pub use local::*;
pub use types::*;

/// 创建使用相对工作目录 `./` 的默认本地执行环境。
pub fn default_env() -> Arc<dyn ExecutionEnv> {
    Arc::new(
        LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: std::path::PathBuf::from("."), ..Default::default() })
            .expect("Default working directory must not be empty"),
    )
}
