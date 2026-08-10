use ormlite::Model as OrmliteModel;
use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    infra::db::{DbPool, Migratable},
};

/// 搜索实体持久化配置。
#[derive(Clone, Debug, Serialize, Deserialize, OrmliteModel)]
#[serde(rename_all = "camelCase")]
#[ormlite(table = "search_engine_configs")]
pub struct SearchEngine {
    /// 搜索实体名称。
    #[ormlite(primary_key)]
    pub engine: String,
    /// 是否在构建 SearchTool 时使用。
    pub enabled: bool,
    /// 搜索实体参数 JSON 字符串。
    pub parameters: String,
}

impl SearchEngine {
    /// 按搜索实体名称查询配置。
    /// @param pool SQLite 连接池。
    /// @param engine 搜索实体名称。
    pub async fn find(pool: &DbPool, engine: &str) -> AppResult<Option<Self>> {
        Self::select()
            .where_bind("engine = ?", engine)
            .fetch_optional(pool)
            .await
            .map_err(Into::into)
    }

    /// 查询全部已保存搜索实体配置。
    /// @param pool SQLite 连接池。
    pub async fn list(pool: &DbPool) -> AppResult<Vec<Self>> {
        Self::select()
            .order_asc("engine")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// 查询全部已启用搜索实体配置。
    /// @param pool SQLite 连接池。
    pub async fn list_enabled(pool: &DbPool) -> AppResult<Vec<Self>> {
        Self::select()
            .where_bind("enabled = ?", true)
            .order_asc("engine")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// 保存搜索实体配置。
    /// @param pool SQLite 连接池。
    /// @param record 搜索实体配置。
    pub async fn save(pool: &DbPool, record: Self) -> AppResult<Self> {
        if Self::find(pool, &record.engine).await?.is_some() {
            return record.update_all_fields(pool).await.map_err(Into::into);
        }
        record.insert(pool).await.map_err(Into::into)
    }
}

impl Migratable for SearchEngine {
    /// 执行搜索实体配置表迁移。
    /// @param pool SQLite 连接池。
    async fn migrate(pool: &DbPool) -> AppResult<()> {
        ormlite::query(
            "CREATE TABLE IF NOT EXISTS search_engine_configs (
                engine TEXT PRIMARY KEY NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT 0,
                parameters TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
