//! 桌面端领域与持久化模型。

pub mod config;
pub mod model;
pub mod preference;
pub mod project;
pub mod provider;
pub mod search;

use crate::{
    error::AppResult,
    infra::db::{DbPool, Migratable},
};

pub use config::*;
pub use model::*;
pub use preference::*;
pub use project::*;
pub use provider::*;
pub use search::*;

/// 执行所有模型迁移。
/// @param pool SQLite 连接池。
pub async fn migrate(pool: &DbPool) -> AppResult<()> {
    Config::migrate(pool).await?;
    Provider::migrate(pool).await?;
    ModelRecord::migrate(pool).await?;
    Preference::migrate(pool).await?;
    Project::migrate(pool).await?;
    SearchEngine::migrate(pool).await?;
    Ok(())
}
