use ai::model::{Auth, AuthProvider, Model};
use async_trait::async_trait;
use ormlite::Model as OrmliteModel;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

use crate::{
    dto::{ProviderInput, ProviderUpdateInput},
    error::AppResult,
    infra::db::{DbPool, Migratable},
    models::ModelRecord,
};

/// 全局默认 Provider 认证提供者。
static DEFAULT_PROVIDER_AUTH_PROVIDER: OnceLock<Arc<ProviderAuthProvider>> = OnceLock::new();

/// 基于桌面端 Provider 配置的模型认证提供者。
pub struct ProviderAuthProvider {
    /// SQLite 连接池。
    pool: &'static DbPool,
}

impl ProviderAuthProvider {
    /// 返回全局默认 Provider 认证提供者。
    /// @param pool SQLite 连接池。
    pub fn global(pool: &'static DbPool) -> Arc<Self> {
        Arc::clone(DEFAULT_PROVIDER_AUTH_PROVIDER.get_or_init(|| Arc::new(Self { pool })))
    }
}

#[async_trait]
impl AuthProvider for ProviderAuthProvider {
    /// 按模型 Provider 查询 API Key 和认证 headers。
    /// @param model 当前请求模型。
    async fn api_key_and_headers<'a>(&'a self, model: &'a Model) -> Option<Auth> {
        match Provider::find(self.pool, &model.provider).await {
            Ok(Some(provider)) => Some(Auth {
                api_key: Some(provider.api_key),
                ..Default::default()
            }),
            Ok(None) => None,
            Err(error) => {
                eprintln!("查询 Provider 认证信息失败: {error:?}");
                None
            }
        }
    }
}

/// 模型 Provider 配置。
#[derive(Clone, Debug, Serialize, Deserialize, OrmliteModel)]
#[serde(rename_all = "camelCase")]
#[ormlite(table = "providers")]
pub struct Provider {
    /// Provider 名称，作为业务主键。
    #[ormlite(primary_key)]
    pub name: String,
    /// AI crate 中注册的 API 标识。
    pub api: String,
    /// Provider 基础 URL。
    pub base_url: String,
    /// Provider API Key。
    pub api_key: String,
}

impl Provider {
    /// 查询 Provider 列表。
    /// @param pool SQLite 连接池。
    pub async fn list(pool: &DbPool) -> AppResult<Vec<Self>> {
        let providers = Self::select().order_asc("name").fetch_all(pool).await?;
        Ok(providers)
    }

    /// 按名称查询 Provider。
    /// @param pool SQLite 连接池。
    /// @param name Provider 名称。
    pub async fn find(pool: &DbPool, name: &str) -> AppResult<Option<Self>> {
        let provider = Self::select()
            .where_bind("name = ?", name)
            .fetch_optional(pool)
            .await?;
        Ok(provider)
    }

    /// 新增 Provider。
    /// @param pool SQLite 连接池。
    /// @param input 新增 Provider 请求。
    pub async fn create(pool: &DbPool, input: ProviderInput) -> AppResult<Self> {
        let provider = Self {
            name: input.name,
            api: input.api,
            base_url: input.base_url,
            api_key: input.api_key,
        };
        provider.insert(pool).await.map_err(Into::into)
    }

    /// 更新 Provider。
    /// @param pool SQLite 连接池。
    /// @param name Provider 名称。
    /// @param input 更新 Provider 请求。
    pub async fn update(pool: &DbPool, name: &str, input: ProviderUpdateInput) -> AppResult<Self> {
        let provider = Self {
            name: name.to_string(),
            api: input.api,
            base_url: input.base_url,
            api_key: input.api_key,
        };
        provider.update_all_fields(pool).await.map_err(Into::into)
    }

    /// 删除 Provider，并级联删除模型记录。
    /// @param pool SQLite 连接池。
    /// @param name Provider 名称。
    pub async fn delete_by_name(pool: &DbPool, name: &str) -> AppResult<()> {
        ModelRecord::delete_by_provider(pool, name).await?;
        ormlite::query("DELETE FROM providers WHERE name = ?")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(())
    }
}

impl Migratable for Provider {
    /// 执行 Provider 表迁移。
    /// @param pool SQLite 连接池。
    async fn migrate(pool: &DbPool) -> AppResult<()> {
        ormlite::query(
            "CREATE TABLE IF NOT EXISTS providers (
				name TEXT PRIMARY KEY NOT NULL,
				api TEXT NOT NULL,
				base_url TEXT NOT NULL,
				api_key TEXT NOT NULL
			)",
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
