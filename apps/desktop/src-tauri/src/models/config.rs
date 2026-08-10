use ormlite::Model as OrmliteModel;
use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    infra::db::{DbPool, Migratable},
};

/// 默认配置主键。
const DEFAULT_CONFIG_KEY: &str = "default";

/// 默认会话存储类型。
const DEFAULT_STORAGE_TYPE: &str = "Sqlite";
/// 会话自动压缩上下文的默认 token 使用比例。
const DEFAULT_COMPACT_RATIO: u8 = 80;

/// 桌面端配置。
#[derive(Clone, Debug, Serialize, Deserialize, OrmliteModel)]
#[serde(rename_all = "camelCase")]
#[ormlite(table = "configs")]
pub struct Config {
    /// 配置主键，目前固定为 default。
    #[ormlite(primary_key)]
    pub config_key: String,
    /// 界面语言。
    pub language: String,
    /// 界面主题。
    pub theme: String,
    /// 工作路径。
    pub path: String,
    /// 会话存储类型标识。
    pub storage_type: String,
    /// 触发会话上下文压缩的 token 使用百分比。
    pub compact_ratio: u8,
}

impl Config {
    /// 获取默认配置，不存在时创建默认配置。
    /// @param pool SQLite 连接池。
    pub async fn get_or_create(pool: &DbPool) -> AppResult<Self> {
        if let Some(config) = Self::select()
            .where_bind("config_key = ?", DEFAULT_CONFIG_KEY)
            .fetch_optional(pool)
            .await?
        {
            return Ok(config);
        }

        let config = Self {
            config_key: DEFAULT_CONFIG_KEY.to_string(),
            language: "zh-CN".to_string(),
            theme: "light".to_string(),
            path: "./data".to_string(),
            storage_type: DEFAULT_STORAGE_TYPE.to_string(),
            compact_ratio: DEFAULT_COMPACT_RATIO,
        };
        config.insert(pool).await.map_err(Into::into)
    }

    /// 保存默认配置。
    /// @param pool SQLite 连接池。
    /// @param input 前端传入的配置数据。
    pub async fn set_config(pool: &DbPool, input: Self) -> AppResult<Self> {
        let mut config = Self::get_or_create(pool).await?;
        config.language = input.language;
        config.theme = input.theme;
        config.path = input.path;
        config.storage_type = input.storage_type;
        config.compact_ratio = input.compact_ratio;
        config.update_all_fields(pool).await.map_err(Into::into)
    }
}

impl Migratable for Config {
    /// 执行配置表迁移。
    /// @param pool SQLite 连接池。
    async fn migrate(pool: &DbPool) -> AppResult<()> {
        ormlite::query(
            "CREATE TABLE IF NOT EXISTS configs (
                config_key TEXT PRIMARY KEY NOT NULL,
                language TEXT NOT NULL DEFAULT '',
                theme TEXT NOT NULL DEFAULT '',
                path TEXT NOT NULL DEFAULT '',
                storage_type TEXT NOT NULL DEFAULT 'Sqlite',
                compact_ratio INTEGER NOT NULL DEFAULT 80
            )",
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}
