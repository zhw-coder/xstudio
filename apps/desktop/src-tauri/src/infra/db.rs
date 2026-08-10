//! 桌面端本地数据库基础设施。

use std::{future::Future, sync::OnceLock};

use crate::{error::AppResult, infra, models};
use ormlite::{sqlite::SqliteConnectOptions, PoolOptions};

/// 桌面端 SQLite 连接池类型。
pub type DbPool = ormlite::Pool<ormlite::sqlite::Sqlite>;

/// 桌面端 SQLite 数据库文件名。
const DB_FILE_NAME: &str = "xstudio.sqlite";

/// 全局 SQLite 连接池。
static DB_POOL: OnceLock<DbPool> = OnceLock::new();

/// 数据库迁移接口。
pub trait Migratable {
    /// 执行数据库迁移。
    /// @param pool SQLite 连接池。
    fn migrate(pool: &DbPool) -> impl Future<Output = AppResult<()>> + Send;
}

/// 获取全局 SQLite 连接池。
/// @param app Tauri 应用句柄，用于解析应用数据目录。
pub async fn pool(_app: &tauri::AppHandle) -> AppResult<&'static DbPool> {
    if let Some(pool) = DB_POOL.get() {
        return Ok(pool);
    }

    let db_path = infra::app_dir()?.join(DB_FILE_NAME);
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true);
    let pool = PoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    models::migrate(&pool).await?;

    let _ = DB_POOL.set(pool);
    Ok(DB_POOL.get().expect("数据库连接池已完成初始化"))
}
