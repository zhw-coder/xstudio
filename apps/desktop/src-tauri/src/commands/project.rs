use crate::{
    dto::{DeleteProjectInput, ProjectOutput, SaveProjectInput},
    error::command_error,
    services,
};

/// 查询全部项目。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn list_projects(app: tauri::AppHandle) -> Result<Vec<ProjectOutput>, String> {
    services::project::list_projects(&app)
        .await
        .map_err(command_error)
}

/// 保存一个项目。
/// @param app Tauri 应用句柄。
/// @param input 项目保存请求。
#[tauri::command]
pub async fn save_project(
    app: tauri::AppHandle,
    input: SaveProjectInput,
) -> Result<ProjectOutput, String> {
    services::project::save_project(&app, input)
        .await
        .map_err(command_error)
}

/// 删除一个项目。
/// @param app Tauri 应用句柄。
/// @param input 项目删除请求。
#[tauri::command]
pub async fn delete_project(
    app: tauri::AppHandle,
    input: DeleteProjectInput,
) -> Result<(), String> {
    services::project::delete_project(&app, input)
        .await
        .map_err(command_error)
}
