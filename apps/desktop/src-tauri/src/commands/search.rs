use crate::{
    dto::{SaveSearchEngineInput, SearchEngineOutput},
    error::command_error,
    services,
};

/// 返回按领域分组的工具库搜索实体名称。
#[tauri::command]
pub fn list_search_engines() -> Vec<Vec<String>> {
    services::search::list_engines()
}

/// 查询搜索实体配置；未保存时返回工具库默认参数。
/// @param app Tauri 应用句柄。
/// @param engine 搜索实体名称。
#[tauri::command]
pub async fn get_search_engine(engine: String) -> Result<SearchEngineOutput, String> {
    services::search::get_engine(&engine).map_err(command_error)
}

/// 查询数据库全部已保存搜索实体配置。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn list_search_configs(app: tauri::AppHandle) -> Result<Vec<SearchEngineOutput>, String> {
    services::search::list_configs(&app)
        .await
        .map_err(command_error)
}

/// 保存搜索实体配置。
/// @param app Tauri 应用句柄。
/// @param input 搜索实体配置请求。
#[tauri::command]
pub async fn save_search_config(
    app: tauri::AppHandle,
    input: SaveSearchEngineInput,
) -> Result<SearchEngineOutput, String> {
    services::search::save_config(&app, input)
        .await
        .map_err(command_error)
}
