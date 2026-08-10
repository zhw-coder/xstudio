use ormlite::{types::Json, Model as OrmliteModel};
use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    infra::db::{DbPool, Migratable},
};

/// 默认偏好配置主键。
const DEFAULT_PREFERENCE_KEY: &str = "default";
/// 工具启用状态值：不启用。
const TOOLS_DISABLED: &str = "0";
/// 工具启用状态值：启用。
const TOOLS_ENABLED: &str = "1";

/// 模型偏好配置。
#[derive(Clone, Debug, Serialize, Deserialize, OrmliteModel)]
#[serde(rename_all = "camelCase")]
#[ormlite(table = "model_preferences")]
pub struct Preference {
    /// 配置主键，目前固定为 default。
    #[ormlite(primary_key)]
    pub preference_key: String,
    /// 模型记录选择。
    pub model_record_selection: String,
    /// 模型思考等级；off 表示不启用 thinking。
    pub model_thinking_level: String,
    /// 工具配置数组，首元素为启用状态（0/1），其余元素为工具名称。
    pub tools: Json<Vec<String>>,
    /// 审批权限：0 表示默认审批，1 表示绕过审批。
    pub approval: i64,
}

impl Preference {
    /// 获取默认模型偏好配置，不存在时创建默认配置。
    /// @param pool SQLite 连接池。
    pub async fn get_or_create(pool: &DbPool) -> AppResult<Self> {
        if let Some(preference) = Self::select()
            .where_bind("preference_key = ?", DEFAULT_PREFERENCE_KEY)
            .fetch_optional(pool)
            .await?
        {
            return Ok(preference);
        }

        let preference = Self {
            preference_key: DEFAULT_PREFERENCE_KEY.to_string(),
            model_record_selection: String::new(),
            model_thinking_level: "off".to_string(),
            tools: Json(vec![TOOLS_DISABLED.to_string()]),
            approval: 0,
        };
        preference.insert(pool).await.map_err(Into::into)
    }

    /// 设置模型记录选择。
    /// @param pool SQLite 连接池。
    /// @param model_record_selection 模型记录选择。
    pub async fn set_model_record_selection(
        pool: &DbPool,
        model_record_selection: String,
    ) -> AppResult<Self> {
        let mut preference = Self::get_or_create(pool).await?;
        preference.model_record_selection = model_record_selection;
        preference.update_all_fields(pool).await.map_err(Into::into)
    }

    /// 设置模型思考等级。
    /// @param pool SQLite 连接池。
    /// @param model_thinking_level 模型思考等级。
    pub async fn set_model_thinking_level(
        pool: &DbPool,
        model_thinking_level: String,
    ) -> AppResult<Self> {
        let mut preference = Self::get_or_create(pool).await?;
        preference.model_thinking_level = model_thinking_level;
        preference.update_all_fields(pool).await.map_err(Into::into)
    }

    /// 设置工具启用状态。
    /// @param pool SQLite 连接池。
    /// @param enabled 是否启用工具。
    pub async fn set_tools_enabled(pool: &DbPool, enabled: bool) -> AppResult<Self> {
        let mut preference = Self::get_or_create(pool).await?;
        preference.tools.0[0] = if enabled {
            TOOLS_ENABLED.to_string()
        } else {
            TOOLS_DISABLED.to_string()
        };
        preference.update_all_fields(pool).await.map_err(Into::into)
    }

    /// 设置完整工具配置。
    /// @param pool SQLite 连接池。
    /// @param tools 工具配置数组。
    pub async fn set_tools(pool: &DbPool, tools: Vec<String>) -> AppResult<Self> {
        let mut preference = Self::get_or_create(pool).await?;
        preference.tools = Json(tools);
        preference.update_all_fields(pool).await.map_err(Into::into)
    }

    /// 设置审批权限。
    /// @param pool SQLite 连接池。
    /// @param approval 审批权限：0 表示默认审批，1 表示绕过审批。
    pub async fn set_approval(pool: &DbPool, approval: i64) -> AppResult<Self> {
        let mut preference = Self::get_or_create(pool).await?;
        preference.approval = approval;
        preference.update_all_fields(pool).await.map_err(Into::into)
    }
}

impl Migratable for Preference {
    /// 执行模型偏好配置表迁移。
    /// @param pool SQLite 连接池。
    async fn migrate(pool: &DbPool) -> AppResult<()> {
        ormlite::query(
            r#"CREATE TABLE IF NOT EXISTS model_preferences (
                preference_key TEXT PRIMARY KEY NOT NULL,
                model_record_selection TEXT NOT NULL DEFAULT '',
                model_thinking_level TEXT NOT NULL DEFAULT 'off',
                tools TEXT NOT NULL DEFAULT '[]',
                approval INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
