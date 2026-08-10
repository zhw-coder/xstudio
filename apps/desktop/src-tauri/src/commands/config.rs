use crate::{error::command_error, models::Config, services};

/// 获取默认配置。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn get_config(app: tauri::AppHandle) -> Result<Config, String> {
    services::config::get_config(&app)
        .await
        .map_err(command_error)
}

/// 保存默认配置。
/// @param app Tauri 应用句柄。
/// @param config 前端传入的配置数据。
#[tauri::command]
pub async fn set_config(app: tauri::AppHandle, config: Config) -> Result<Config, String> {
    services::config::set_config(&app, config)
        .await
        .map_err(command_error)
}

/// 在系统文件管理器中打开应用数据目录。
#[tauri::command]
pub async fn open_app_dir() -> Result<(), String> {
    services::config::open_app_dir().await.map_err(command_error)
}
