use crate::{
    dto::{
        AllProviderModelsOutput, FetchModelsInput, PreferenceApprovalInput,
        PreferenceToolEnabledInput, PreferenceToolInput, PreferenceToolsInput, ProviderInput,
        ProviderModelIdsOutput, ProviderUpdateInput,
    },
    error::command_error,
    models::{ModelRecord, Preference, Provider},
    services,
};

/// 查询 Provider 列表。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn list_providers(app: tauri::AppHandle) -> Result<Vec<Provider>, String> {
    services::models::list_providers(&app)
        .await
        .map_err(command_error)
}

/// 新增 Provider。
/// @param app Tauri 应用句柄。
/// @param input Provider 输入。
#[tauri::command]
pub async fn create_provider(
    app: tauri::AppHandle,
    input: ProviderInput,
) -> Result<Provider, String> {
    services::models::create_provider(&app, input)
        .await
        .map_err(command_error)
}

/// 更新 Provider。
/// @param app Tauri 应用句柄。
/// @param name Provider 名称。
/// @param input Provider 更新输入。
#[tauri::command]
pub async fn update_provider(
    app: tauri::AppHandle,
    name: String,
    input: ProviderUpdateInput,
) -> Result<Provider, String> {
    services::models::update_provider(&app, &name, input)
        .await
        .map_err(command_error)
}

/// 删除 Provider。
/// @param app Tauri 应用句柄。
/// @param name Provider 名称。
#[tauri::command]
pub async fn delete_provider(app: tauri::AppHandle, name: String) -> Result<(), String> {
    services::models::delete_provider(&app, &name)
        .await
        .map_err(command_error)
}

/// 查询指定 Provider 的本地模型列表。
/// @param app Tauri 应用句柄。
/// @param provider_name Provider 名称。
#[tauri::command]
pub async fn list_models_by_provider(
    app: tauri::AppHandle,
    provider_name: String,
) -> Result<Vec<ModelRecord>, String> {
    services::models::list_models_by_provider(&app, &provider_name)
        .await
        .map_err(command_error)
}

/// 同步更新指定 Provider 的本地模型列表。
/// @param app Tauri 应用句柄。
/// @param provider_name Provider 名称。
#[tauri::command]
pub async fn sync_models_by_provider(
    app: tauri::AppHandle,
    provider_name: String,
    models: Vec<ModelRecord>,
) -> Result<Vec<ModelRecord>, String> {
    services::models::sync_models_by_provider(&app, &provider_name, models)
        .await
        .map_err(command_error)
}

/// 获取所有 Provider 模型完整数据映射和 AI API Provider 列表。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn all_provider_models_map(
    app: tauri::AppHandle,
) -> Result<AllProviderModelsOutput, String> {
    services::models::all_provider_models_map(&app)
        .await
        .map_err(command_error)
}

/// 获取所有 Provider 名称和模型 id 映射。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn provider_model_ids_map(
    app: tauri::AppHandle,
) -> Result<ProviderModelIdsOutput, String> {
    services::models::provider_model_ids_map(&app)
        .await
        .map_err(command_error)
}

/// 通过 AI 库 Provider models 接口获取远端模型列表，不写入本地数据库。
/// @param input 拉取远端模型列表请求。
#[tauri::command]
pub async fn fetch_models_from_provider(
    input: FetchModelsInput,
) -> Result<Vec<ModelRecord>, String> {
    services::models::fetch_models_from_provider(input)
        .await
        .map_err(command_error)
}

/// 获取 AI 库模型思考档位经过 serde rename 后的字符串列表。
#[tauri::command]
pub fn model_thinking_levels() -> Result<Vec<String>, String> {
    services::models::model_thinking_levels().map_err(command_error)
}

/// 设置模型记录选择。
/// @param app Tauri 应用句柄。
/// @param model_record_selection 模型记录选择。
#[tauri::command]
pub async fn set_model_record_selection(
    app: tauri::AppHandle,
    model_record_selection: String,
) -> Result<Preference, String> {
    services::models::set_model_record_selection(&app, model_record_selection)
        .await
        .map_err(command_error)
}

/// 设置模型思考等级。
/// @param app Tauri 应用句柄。
/// @param model_thinking_level 模型思考等级。
#[tauri::command]
pub async fn set_model_thinking_level(
    app: tauri::AppHandle,
    model_thinking_level: String,
) -> Result<Preference, String> {
    services::models::set_model_thinking_level(&app, model_thinking_level)
        .await
        .map_err(command_error)
}

/// 返回工具库支持的全部工具名称。
#[tauri::command]
pub fn list_tool_names() -> Vec<String> {
    services::models::tool_names()
}

/// 添加工具名称。
/// @param app Tauri 应用句柄。
/// @param input 工具请求。
#[tauri::command]
pub async fn add_preference_tool(
    app: tauri::AppHandle,
    input: PreferenceToolInput,
) -> Result<Preference, String> {
    services::models::add_tool(&app, &input.tool)
        .await
        .map_err(command_error)
}

/// 删除工具名称。
/// @param app Tauri 应用句柄。
/// @param input 工具请求。
#[tauri::command]
pub async fn remove_preference_tool(
    app: tauri::AppHandle,
    input: PreferenceToolInput,
) -> Result<Preference, String> {
    services::models::remove_tool(&app, &input.tool)
        .await
        .map_err(command_error)
}

/// 设置完整工具配置。
/// @param app Tauri 应用句柄。
/// @param input 工具请求。
#[tauri::command]
pub async fn set_preference_tools(
    app: tauri::AppHandle,
    input: PreferenceToolsInput,
) -> Result<Preference, String> {
    services::models::set_tools(&app, input.tools)
        .await
        .map_err(command_error)
}

/// 设置工具启用状态。
/// @param app Tauri 应用句柄。
/// @param input 工具启用状态请求。
#[tauri::command]
pub async fn set_preference_tool_enabled(
    app: tauri::AppHandle,
    input: PreferenceToolEnabledInput,
) -> Result<Preference, String> {
    services::models::set_tools_enabled(&app, input.enabled)
        .await
        .map_err(command_error)
}

/// 设置审批权限。
/// @param app Tauri 应用句柄。
/// @param input 审批权限请求。
#[tauri::command]
pub async fn set_preference_approval(
    app: tauri::AppHandle,
    input: PreferenceApprovalInput,
) -> Result<Preference, String> {
    services::models::set_approval(&app, input.approval)
        .await
        .map_err(command_error)
}
