use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::{ModelRecord, Preference, Provider};

/// 新增 Provider 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    /// Provider 名称。
    pub name: String,
    /// AI crate 中注册的 API 标识。
    pub api: String,
    /// Provider 基础 URL。
    pub base_url: String,
    /// Provider API Key。
    pub api_key: String,
}

/// 更新 Provider 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUpdateInput {
    /// AI crate 中注册的 API 标识。
    pub api: String,
    /// Provider 基础 URL。
    pub base_url: String,
    /// Provider API Key。
    pub api_key: String,
}

/// 拉取远端模型列表请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchModelsInput {
    /// Provider 名称。
    pub name: String,
    /// AI crate 中注册的 API 标识。
    pub api: String,
    /// Provider 基础 URL。
    pub base_url: String,
    /// Provider API Key。
    pub api_key: String,
}

/// Provider 和完整模型列表。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModels {
    /// Provider 配置。
    pub provider: Provider,
    /// Provider 下完整模型记录列表。
    pub models: Vec<ModelRecord>,
}

/// 所有 Provider 模型数据和 AI API Provider 列表。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllProviderModelsOutput {
    /// Provider 名称到完整数据的映射。
    pub provider_models_map: ProviderModelsMap,
    /// AI crate 中已注册的 API Provider 标识列表。
    pub api_provider_apis: Vec<String>,
}

/// 所有 Provider 模型 id 映射和模型思考档位列表。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelIdsOutput {
    /// Provider 名称到模型 id 列表的映射。
    pub provider_model_ids_map: ProviderModelIdsMap,
    /// Provider:ModelId 到上下文最大 token 数的映射。
    pub provider_model_tokens_map: ProviderModelTokensMap,
    /// 模型思考档位列表。
    pub model_thinking_levels: Vec<String>,
    /// 模型偏好配置。
    pub preference: Preference,
}

/// 设置完整工具偏好请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceToolsInput {
    /// 工具配置数组，首元素为启用状态（0/1），其余元素为工具名称。
    pub tools: Vec<String>,
}

/// 更新单个工具选择状态请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceToolInput {
    /// 工具名称。
    pub tool: String,
}

/// 设置工具启用状态请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceToolEnabledInput {
    /// 是否启用工具。
    pub enabled: bool,
}

/// 设置审批权限请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceApprovalInput {
    /// 审批权限：0 表示默认审批，1 表示绕过审批。
    pub approval: i64,
}

/// Provider 名称到完整数据的映射。
pub type ProviderModelsMap = HashMap<String, ProviderModels>;

/// Provider 名称到模型 id 列表的映射。
pub type ProviderModelIdsMap = HashMap<String, Vec<String>>;

/// Provider:ModelId 到上下文最大 token 数的映射。
pub type ProviderModelTokensMap = HashMap<String, u64>;
