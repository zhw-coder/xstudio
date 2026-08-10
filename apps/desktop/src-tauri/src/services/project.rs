use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    context,
    dto::{DeleteProjectInput, ProjectOutput, SaveProjectInput},
    error::AppResult,
    infra::db,
    models::Project,
};

/// 查询全部项目。
/// @param app Tauri 应用句柄。
pub async fn list_projects(app: &tauri::AppHandle) -> AppResult<Vec<ProjectOutput>> {
    Ok(Project::list(db::pool(app).await?)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// 获取最近更新的项目路径。
/// @param app Tauri 应用句柄。
pub async fn latest_project_path(app: &tauri::AppHandle) -> AppResult<Option<String>> {
    Ok(Project::list(db::pool(app).await?)
        .await?
        .into_iter()
        .next()
        .map(|project| project.path))
}

/// 保存一个项目，并由后端写入当前更新时间。
/// @param app Tauri 应用句柄。
/// @param input 项目保存请求。
pub async fn save_project(
    app: &tauri::AppHandle,
    input: SaveProjectInput,
) -> AppResult<ProjectOutput> {
    let project = Project::save(db::pool(app).await?, input.path, now_millis()).await?;
    context::reset_async(&project.path).await?;
    Ok(project.into())
}

/// 按路径删除一个项目。
/// @param app Tauri 应用句柄。
/// @param input 项目删除请求。
pub async fn delete_project(app: &tauri::AppHandle, input: DeleteProjectInput) -> AppResult<()> {
    Project::delete_by_path(db::pool(app).await?, &input.path).await
}

/// 获取当前 Unix 毫秒时间戳。
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

impl From<Project> for ProjectOutput {
    /// 将持久化模型转换为客户端输出。
    /// @param project 项目持久化记录。
    fn from(project: Project) -> Self {
        Self {
            path: project.path,
            updated_at: project.updated_at,
        }
    }
}
