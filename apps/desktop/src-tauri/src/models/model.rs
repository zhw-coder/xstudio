use std::collections::HashMap;

use ai::agent::harness::SessionModelSelection;
use ai::model::Model;
use ormlite::Model as OrmliteModel;
use serde::{Deserialize, Serialize};

use crate::{
    dto::ProviderModelIdsMap,
    error::{AppError, AppResult},
    infra::db::{DbPool, Migratable},
};

/// Provider 模型持久化记录。
#[derive(Clone, Debug, Serialize, Deserialize, OrmliteModel)]
#[serde(rename_all = "camelCase")]
#[ormlite(table = "provider_models")]
pub struct ModelRecord {
    /// 组合主键，格式为 provider_name:model_id。
    #[ormlite(primary_key)]
    pub record_key: String,
    /// Provider 名称。
    pub provider_name: String,
    /// 模型 id。
    pub model_id: String,
    /// 是否启用该模型。
    pub status: bool,
    /// ai::model::Model 的 JSON 快照。
    pub model_json: String,
}

impl ModelRecord {
    /// 查询指定 Provider 的模型列表。
    /// @param pool SQLite 连接池。
    /// @param provider_name Provider 名称。
    pub async fn list_models(pool: &DbPool, provider_name: &str) -> AppResult<Vec<ModelRecord>> {
        Self::select()
            .where_bind("provider_name = ?", provider_name)
            .order_asc("model_id")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// 覆盖保存指定 Provider 的模型列表。
    /// @param pool SQLite 连接池。
    /// @param provider_name Provider 名称。
    /// @param records 完整模型记录列表。
    pub async fn replace_models(
        pool: &DbPool,
        provider_name: &str,
        records: Vec<ModelRecord>,
    ) -> AppResult<Vec<ModelRecord>> {
        Self::delete_by_provider(pool, provider_name).await?;
        for record in records {
            let record = record.normalized(provider_name);
            record.insert(pool).await?;
        }
        Self::list_models(pool, provider_name).await
    }

    /// 删除指定 Provider 的全部模型记录。
    /// @param pool SQLite 连接池。
    /// @param provider_name Provider 名称。
    pub async fn delete_by_provider(pool: &DbPool, provider_name: &str) -> AppResult<()> {
        ormlite::query("DELETE FROM provider_models WHERE provider_name = ?")
            .bind(provider_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// 构造 Provider 名称到模型 id 列表的映射。
    /// @param pool SQLite 连接池。
    pub async fn all_model_ids_map(pool: &DbPool) -> AppResult<ProviderModelIdsMap> {
        let records = Self::select()
            .where_bind("status = ?", true)
            .order_asc("provider_name")
            .order_asc("model_id")
            .fetch_all(pool)
            .await?;
        let mut output = HashMap::new();
        for record in records {
            output
                .entry(record.provider_name)
                .or_insert_with(Vec::new)
                .push(record.model_id);
        }
        Ok(output)
    }

    /// 构造 Provider:ModelId 到上下文最大 token 数的映射。
    /// @param pool SQLite 连接池。
    pub async fn all_model_tokens_map(
        pool: &DbPool,
    ) -> AppResult<crate::dto::ProviderModelTokensMap> {
        let records = Self::select()
            .where_bind("status = ?", true)
            .order_asc("provider_name")
            .order_asc("model_id")
            .fetch_all(pool)
            .await?;
        let mut output = HashMap::new();
        for record in records {
            let model: Model = serde_json::from_str(&record.model_json)?;
            output.insert(record.record_key, model.context_window);
        }
        Ok(output)
    }

    /// 根据会话模型选择查询 AI 模型元数据。
    /// @param pool SQLite 连接池。
    /// @param selection 会话模型选择。
    pub async fn find_model_by_selection(
        pool: &DbPool,
        selection: &SessionModelSelection,
    ) -> AppResult<Model> {
        let record_key = record_key(&selection.provider, &selection.model_id);
        Self::find_model_by_record_key(pool, &record_key).await
    }

    /// 根据模型记录主键查询 AI 模型元数据。
    /// @param pool SQLite 连接池。
    /// @param record_key 模型记录主键。
    pub async fn find_model_by_record_key(pool: &DbPool, record_key: &str) -> AppResult<Model> {
        let Some(record) = Self::select()
            .where_bind("record_key = ?", record_key)
            .where_bind("status = ?", true)
            .fetch_optional(pool)
            .await?
        else {
            return Err(AppError::SessionModelNotFound {
                record_key: record_key.to_string(),
            });
        };
        serde_json::from_str(&record.model_json).map_err(Into::into)
    }

    /// 无条件查询一条 AI 模型元数据。
    /// @param pool SQLite 连接池。
    pub async fn find_any_model(pool: &DbPool) -> AppResult<Model> {
        let Some(record) = Self::select().fetch_optional(pool).await? else {
            return Err(AppError::SessionModelNotFound {
                record_key: String::new(),
            });
        };
        serde_json::from_str(&record.model_json).map_err(Into::into)
    }

    /// 从 ai::model::Model 构造持久化记录。
    /// @param provider_name Provider 名称。
    /// @param model AI 模型元数据。
    pub fn from_model(provider_name: &str, model: &Model) -> AppResult<Self> {
        let model_json = serde_json::to_string(model)?;
        Ok(Self {
            record_key: record_key(provider_name, &model.id),
            provider_name: provider_name.to_string(),
            model_id: model.id.to_string(),
            status: true,
            model_json,
        })
    }

    /// 归一化 Provider 相关字段，避免信任前端传入的组合主键。
    /// @param provider_name Provider 名称。
    fn normalized(mut self, provider_name: &str) -> Self {
        self.provider_name = provider_name.to_string();
        self.record_key = record_key(provider_name, &self.model_id);
        self
    }
}

impl Migratable for ModelRecord {
    /// 执行 Provider 模型记录表迁移。
    /// @param pool SQLite 连接池。
    async fn migrate(pool: &DbPool) -> AppResult<()> {
        ormlite::query(
            "CREATE TABLE IF NOT EXISTS provider_models (
				record_key TEXT PRIMARY KEY NOT NULL,
				provider_name TEXT NOT NULL,
				model_id TEXT NOT NULL,
                status BOOLEAN NOT NULL DEFAULT TRUE,
				model_json TEXT NOT NULL,
				FOREIGN KEY(provider_name) REFERENCES providers(name) ON DELETE CASCADE
			)",
        )
        .execute(pool)
        .await?;

        ormlite::query(
            "CREATE INDEX IF NOT EXISTS idx_provider_models_provider_name
				ON provider_models(provider_name)",
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}

/// 构造 Provider 模型记录主键。
/// @param provider_name Provider 名称。
/// @param model_id 模型 id。
fn record_key(provider_name: &str, model_id: &str) -> String {
    format!("{provider_name}:{model_id}")
}
