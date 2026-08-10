use crate::{
    dto::{
        DeleteTemplateFileInput, SaveTemplateFileInput, SetSkillDisableModelInvocationInput,
        SkillFileOutput, TemplateDir, TemplateFileListOutput, TemplateFileOutput,
    },
    error::command_error,
    services,
};

/// 查询应用模板目录中的全部模板文件。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn list_template_files(
    app: tauri::AppHandle,
) -> Result<Vec<TemplateFileListOutput>, String> {
    services::resources::list_template_files(&app)
        .await
        .map_err(command_error)
}

/// 查询当前项目中的全部 Skill 文件。
/// @param app Tauri 应用句柄。
#[tauri::command]
pub async fn list_skill_files(app: tauri::AppHandle) -> Result<Vec<SkillFileOutput>, String> {
    services::resources::list_skill_files(&app)
        .await
        .map_err(command_error)
}

/// 更新 Skill 是否允许模型自主调用。
/// @param app Tauri 应用句柄。
/// @param input Skill 更新请求。
#[tauri::command]
pub async fn set_skill_disable_model_invocation(
    app: tauri::AppHandle,
    input: SetSkillDisableModelInvocationInput,
) -> Result<(), String> {
    services::resources::set_skill_disable_model_invocation(
        &app,
        &input.path,
        input.disable_model_invocation,
    )
    .await
    .map_err(command_error)
}

/// 读取一个模板文件。
/// @param app Tauri 应用句柄。
/// @param name 模板名，不包含 `.md` 扩展名。
/// @param dir 模板目录。
#[tauri::command]
pub async fn get_template_file(
    app: tauri::AppHandle,
    name: String,
    dir: TemplateDir,
) -> Result<TemplateFileOutput, String> {
    services::resources::get_template_file(&app, &name, &dir)
        .await
        .map_err(command_error)
}

/// 保存一个模板文件。
/// @param app Tauri 应用句柄。
/// @param input 模板文件保存请求。
#[tauri::command]
pub async fn save_template_file(
    app: tauri::AppHandle,
    input: SaveTemplateFileInput,
) -> Result<(), String> {
    services::resources::save_template_file(&app, input)
        .await
        .map_err(command_error)
}

/// 删除一个模板文件。
/// @param app Tauri 应用句柄。
/// @param input 模板文件删除请求。
#[tauri::command]
pub async fn delete_template_file(
    app: tauri::AppHandle,
    input: DeleteTemplateFileInput,
) -> Result<(), String> {
    services::resources::delete_template_file(&app, &input.name, &input.dir)
        .await
        .map_err(command_error)
}
