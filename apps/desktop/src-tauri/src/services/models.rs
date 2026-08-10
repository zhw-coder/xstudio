use std::sync::Arc;

use crate::{
    dto::{
        AllProviderModelsOutput, FetchModelsInput, ProviderInput, ProviderModelIdsOutput,
        ProviderModels, ProviderModelsMap, ProviderUpdateInput,
    },
    error::{AppError, AppResult},
    infra::db,
    models::{ModelRecord, Preference, Provider, ProviderAuthProvider},
};
use ai::{
    agent::harness::SessionModelSelection,
    model::{ApiRegistry, Auth, AuthProvider, Model, StreamOptions, ThinkingLevel},
};
use ormlite::Model as OrmliteModel;

/// 查询 Provider 列表。
/// @param app Tauri 应用句柄。
pub async fn list_providers(app: &tauri::AppHandle) -> AppResult<Vec<Provider>> {
    Provider::list(db::pool(app).await?).await
}

/// 新增 Provider。
/// @param app Tauri 应用句柄。
/// @param input Provider 输入。
pub async fn create_provider(app: &tauri::AppHandle, input: ProviderInput) -> AppResult<Provider> {
    Provider::create(db::pool(app).await?, input).await
}

/// 更新 Provider。
/// @param app Tauri 应用句柄。
/// @param name Provider 名称。
/// @param input Provider 更新输入。
pub async fn update_provider(
    app: &tauri::AppHandle,
    name: &str,
    input: ProviderUpdateInput,
) -> AppResult<Provider> {
    Provider::update(db::pool(app).await?, name, input).await
}

/// 删除 Provider。
/// @param app Tauri 应用句柄。
/// @param name Provider 名称。
pub async fn delete_provider(app: &tauri::AppHandle, name: &str) -> AppResult<()> {
    Provider::delete_by_name(db::pool(app).await?, name).await
}

/// 通过 Provider 查询本地模型列表。
/// @param app Tauri 应用句柄。
/// @param provider_name Provider 名称。
pub async fn list_models_by_provider(
    app: &tauri::AppHandle,
    provider_name: &str,
) -> AppResult<Vec<ModelRecord>> {
    ModelRecord::list_models(db::pool(app).await?, provider_name).await
}

/// 构造 Provider 认证提供者。
/// @param app Tauri 应用句柄。
pub async fn auth_provider(app: &tauri::AppHandle) -> AppResult<Arc<dyn AuthProvider>> {
    Ok(ProviderAuthProvider::global(db::pool(app).await?))
}

/// 通过 AI 库 Provider models 接口获取远端模型列表。
/// @param input 拉取远端模型列表请求。
pub async fn fetch_models_from_provider(input: FetchModelsInput) -> AppResult<Vec<ModelRecord>> {
    let api_provider = ApiRegistry::global()
        .get(&input.api)
        .ok_or_else(|| AppError::ApiProviderNotFound(input.api.to_string()))?;
    let auth = Auth {
        api_key: Some(input.api_key),
        ..Default::default()
    };
    let models = api_provider
        .models(
            &input.name,
            &input.base_url,
            &StreamOptions::default(),
            &auth,
        )
        .await?;
    models
        .iter()
        .map(|model| ModelRecord::from_model(&input.name, model))
        .collect()
}

/// 获取模型思考档位字符串列表；off 表示不启用 thinking。
pub fn model_thinking_levels() -> AppResult<Vec<String>> {
    /// 按前端展示和模型元数据约定输出的思考档位顺序。
    const LEVELS: [ThinkingLevel; 5] = [
        ThinkingLevel::Minimal,
        ThinkingLevel::Low,
        ThinkingLevel::Medium,
        ThinkingLevel::High,
        ThinkingLevel::XHigh,
    ];
    let mut levels: Vec<String> = serde_json::from_value(serde_json::to_value(LEVELS)?)?;
    levels.insert(0, "off".to_string());
    Ok(levels)
}

/// 将字符串转换为可选模型思考等级；off 转为 None。
/// @param level 模型思考等级字符串。
pub fn string_to_thinking_level(level: &str) -> AppResult<Option<ThinkingLevel>> {
    if level == "off" {
        return Ok(None);
    }

    serde_json::from_value(serde_json::Value::String(level.to_string())).map_err(Into::into)
}

/// 同步更新指定 Provider 的本地模型列表。
/// @param app Tauri 应用句柄。
/// @param provider_name Provider 名称。
/// @param models 前端传入的完整模型记录列表。
pub async fn sync_models_by_provider(
    app: &tauri::AppHandle,
    provider_name: &str,
    models: Vec<ModelRecord>,
) -> AppResult<Vec<ModelRecord>> {
    let pool = db::pool(app).await?;
    ModelRecord::replace_models(pool, provider_name, models).await
}

/// 获取所有 Provider 模型完整数据映射和 AI API Provider 列表。
/// @param app Tauri 应用句柄。
pub async fn all_provider_models_map(app: &tauri::AppHandle) -> AppResult<AllProviderModelsOutput> {
    let pool = db::pool(app).await?;
    let providers = Provider::list(pool).await?;
    let mut provider_models_map = ProviderModelsMap::new();
    for provider in providers {
        let models = ModelRecord::list_models(pool, &provider.name).await?;
        provider_models_map.insert(
            provider.name.to_string(),
            ProviderModels { provider, models },
        );
    }
    let api_provider_apis = ApiRegistry::global().apis();
    Ok(AllProviderModelsOutput {
        provider_models_map,
        api_provider_apis,
    })
}

/// 获取所有 Provider 名称和模型 id 映射。
/// @param app Tauri 应用句柄。
pub async fn provider_model_ids_map(app: &tauri::AppHandle) -> AppResult<ProviderModelIdsOutput> {
    let pool = db::pool(app).await?;
    let provider_model_ids_map = ModelRecord::all_model_ids_map(pool).await?;
    let provider_model_tokens_map = ModelRecord::all_model_tokens_map(pool).await?;
    let model_thinking_levels = model_thinking_levels()?;
    let preference = Preference::get_or_create(pool).await?;
    Ok(ProviderModelIdsOutput {
        provider_model_ids_map,
        provider_model_tokens_map,
        model_thinking_levels,
        preference,
    })
}

/// 根据会话模型选择查询 AI 模型元数据。
/// @param app Tauri 应用句柄。
/// @param selection 会话模型选择。
pub async fn find_model_by_selection(
    app: &tauri::AppHandle,
    selection: &SessionModelSelection,
) -> AppResult<Model> {
    ModelRecord::find_model_by_selection(db::pool(app).await?, selection).await
}

/// 根据模型记录主键查询 AI 模型元数据。
/// @param app Tauri 应用句柄。
/// @param record_key 模型记录主键。
pub async fn find_model_by_record_key(
    app: &tauri::AppHandle,
    record_key: &str,
) -> AppResult<Model> {
    ModelRecord::find_model_by_record_key(db::pool(app).await?, record_key).await
}

/// 无条件查询一条 AI 模型元数据。
/// @param app Tauri 应用句柄。
pub async fn find_any_model(app: &tauri::AppHandle) -> AppResult<Model> {
    ModelRecord::find_any_model(db::pool(app).await?).await
}

/// 设置模型记录选择。
/// @param app Tauri 应用句柄。
/// @param model_record_selection 模型记录选择。
pub async fn set_model_record_selection(
    app: &tauri::AppHandle,
    model_record_selection: String,
) -> AppResult<Preference> {
    Preference::set_model_record_selection(db::pool(app).await?, model_record_selection).await
}

/// 设置模型思考等级。
/// @param app Tauri 应用句柄。
/// @param model_thinking_level 模型思考等级。
pub async fn set_model_thinking_level(
    app: &tauri::AppHandle,
    model_thinking_level: String,
) -> AppResult<Preference> {
    let _ = string_to_thinking_level(&model_thinking_level)?;
    Preference::set_model_thinking_level(db::pool(app).await?, model_thinking_level).await
}

/// 获取模型偏好配置。
/// @param app Tauri 应用句柄。
pub async fn get_preference(app: &tauri::AppHandle) -> AppResult<Preference> {
    Preference::get_or_create(db::pool(app).await?).await
}

/// 返回全部内置工具名称。
pub fn tool_names() -> Vec<String> {
    tool::ToolRegistry::global().names()
}

/// 添加工具名称。
/// @param app Tauri 应用句柄。
/// @param tool_name 工具名称。
pub async fn add_tool(app: &tauri::AppHandle, tool_name: &str) -> AppResult<Preference> {
    validate_tool_name(tool_name)?;
    let pool = db::pool(app).await?;
    let mut preference = Preference::get_or_create(pool).await?;
    let tools = &mut preference.tools.0;
    if !tools.iter().skip(1).any(|tool| tool == tool_name) {
        tools.push(tool_name.to_string());
    }
    preference.update_all_fields(pool).await.map_err(Into::into)
}

/// 删除工具名称。
/// @param app Tauri 应用句柄。
/// @param tool_name 工具名称。
pub async fn remove_tool(app: &tauri::AppHandle, tool_name: &str) -> AppResult<Preference> {
    validate_tool_name(tool_name)?;
    let pool = db::pool(app).await?;
    let mut preference = Preference::get_or_create(pool).await?;
    preference
        .tools
        .0
        .retain(|tool| tool == "0" || tool == "1" || tool != tool_name);
    preference.update_all_fields(pool).await.map_err(Into::into)
}

/// 设置工具启用状态。
/// @param app Tauri 应用句柄。
/// @param enabled 是否启用工具。
pub async fn set_tools_enabled(app: &tauri::AppHandle, enabled: bool) -> AppResult<Preference> {
    Preference::set_tools_enabled(db::pool(app).await?, enabled).await
}

/// 设置完整工具配置。
/// @param app Tauri 应用句柄。
/// @param tools 工具配置数组。
pub async fn set_tools(app: &tauri::AppHandle, tools: Vec<String>) -> AppResult<Preference> {
    for tool_name in tools.iter().skip(1) {
        validate_tool_name(tool_name)?;
    }
    Preference::set_tools(db::pool(app).await?, tools).await
}

/// 设置审批权限。
/// @param app Tauri 应用句柄。
/// @param approval 审批权限：0 表示默认审批，1 表示绕过审批。
pub async fn set_approval(app: &tauri::AppHandle, approval: i64) -> AppResult<Preference> {
    Preference::set_approval(db::pool(app).await?, approval).await
}

/// 校验工具名称是否为工具库内置工具。
/// @param tool_name 工具名称。
pub fn validate_tool_name(tool_name: &str) -> AppResult<()> {
    if tool::ToolRegistry::global()
        .names()
        .iter()
        .any(|name| name == tool_name)
    {
        return Ok(());
    }
    Err(AppError::ToolNotFound(tool_name.to_string()))
}
