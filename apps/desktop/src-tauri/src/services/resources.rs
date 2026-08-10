use std::collections::HashMap;

use ai::agent::{
    env::{ExecutionEnv, FileError, FileKind},
    harness::{load_prompt_templates, load_skills},
};
use serde::Serialize;

use crate::{
    context,
    dto::{
        SaveTemplateFileInput, SkillFileOutput, TemplateDir, TemplateFileListOutput,
        TemplateFileOutput,
    },
    error::{AppError, AppResult},
    infra,
};

/// 全局和项目中存放提示词模板的子目录名。
pub(crate) const TEMPLATES_DIR_NAME: &str = "templates";
/// 项目中存放 Skill 的子目录名。
pub(crate) const SKILLS_DIR_NAME: &str = "skills";

/// 获取资源目录在列表中的排序优先级。
/// @param dir 资源目录类型。
fn resource_dir_order(dir: &TemplateDir) -> u8 {
    match dir {
        TemplateDir::Global => 0,
        TemplateDir::Project => 1,
    }
}

/// 返回指定类型的资源目录，并确保目录存在。
/// @param app Tauri 应用句柄。
/// @param dir 资源目录类型。
/// @param child_dir 资源子目录名。
async fn resource_dir(
    app: &tauri::AppHandle,
    dir: &TemplateDir,
    child_dir: &str,
) -> AppResult<String> {
    let env = context::context_async(app).await?.env.read().await.clone();
    let directory = match dir {
        TemplateDir::Global => env.join_path(&infra::app_dir()?.display().to_string(), child_dir),
        TemplateDir::Project => env.join_path(env.cwd(), child_dir),
    };
    env.create_dir(&directory, true).await.map_err(file_error)?;
    Ok(directory)
}

/// 返回全局和项目资源目录，并确保目录存在。
/// @param app Tauri 应用句柄。
/// @param child_dir 资源子目录名。
pub(crate) async fn resource_dirs(
    app: &tauri::AppHandle,
    child_dir: &str,
) -> AppResult<[(String, TemplateDir); 2]> {
    let global_dir = resource_dir(app, &TemplateDir::Global, child_dir).await?;
    let project_dir = resource_dir(app, &TemplateDir::Project, child_dir).await?;
    Ok([
        (global_dir, TemplateDir::Global),
        (project_dir, TemplateDir::Project),
    ])
}

/// 查询全局和项目模板目录中的模板文件；项目同名模板覆盖全局模板。
/// @param app Tauri 应用句柄。
pub async fn list_template_files(app: &tauri::AppHandle) -> AppResult<Vec<TemplateFileListOutput>> {
    let env = context::context_async(app).await?.env.read().await.clone();
    let mut files = HashMap::new();
    for (directory, dir) in resource_dirs(app, TEMPLATES_DIR_NAME).await? {
        let entries = env.list_dir(&directory).await.map_err(file_error)?;
        files.extend(
            entries
                .into_iter()
                .filter(|entry| entry.kind == FileKind::File && entry.name.ends_with(".md"))
                .map(|entry| {
                    let name = entry.name.trim_end_matches(".md").to_string();
                    (
                        name.clone(),
                        TemplateFileListOutput::list_item(name, dir.clone()),
                    )
                }),
        );
    }
    let mut files = files.into_values().collect::<Vec<_>>();
    files.sort_by(|left, right| {
        resource_dir_order(&left.dir)
            .cmp(&resource_dir_order(&right.dir))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(files)
}

/// 查询当前项目 Skill 目录中按加载规则可发现的 Skill 文件。
/// @param app Tauri 应用句柄。
pub async fn list_skill_files(app: &tauri::AppHandle) -> AppResult<Vec<SkillFileOutput>> {
    let env = context::context_async(app).await?.env.read().await.clone();
    let mut files = HashMap::new();
    for (directory, dir) in resource_dirs(app, SKILLS_DIR_NAME).await? {
        let (skills, diagnostics) = load_skills(env.as_ref(), &[directory]).await;
        if let Some(diagnostic) = diagnostics.first() {
            eprintln!("Skill 文件加载诊断: {diagnostic:?}");
        }
        files.extend(skills.into_iter().map(|skill| {
            let name = skill.name;
            (
                name.clone(),
                SkillFileOutput {
                    name,
                    dir: dir.clone(),
                    path: skill.file_path,
                    description: skill.description,
                    disable_model_invocation: skill.disable_model_invocation,
                },
            )
        }));
    }
    let mut files = files.into_values().collect::<Vec<_>>();
    files.sort_by(|left, right| {
        resource_dir_order(&left.dir)
            .cmp(&resource_dir_order(&right.dir))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(files)
}

/// 更新 Skill frontmatter 中的 `disable-model-invocation` 值。
/// @param app Tauri 应用句柄。
/// @param path Skill 文件绝对路径。
/// @param disabled 是否禁止模型自主调用。
pub async fn set_skill_disable_model_invocation(
    app: &tauri::AppHandle,
    path: &str,
    disabled: bool,
) -> AppResult<()> {
    let env = context::context_async(app).await?.env.read().await.clone();
    validate_skill_file_path(app, env.as_ref(), path).await?;
    clear_agent_harness_cache().await?;
    update_skill_disable_model_invocation(env.as_ref(), path, disabled).await?;
    Ok(())
}

/// 读取一个模板文件。
/// @param app Tauri 应用句柄。
/// @param name 模板名，不包含 `.md` 扩展名。
/// @param dir 客户端选择的模板目录类型。
pub async fn get_template_file(
    app: &tauri::AppHandle,
    name: &str,
    dir: &TemplateDir,
) -> AppResult<TemplateFileOutput> {
    let env = context::context_async(app).await?.env.read().await.clone();
    validate_template_name(name)?;
    let directory = resource_dir(app, dir, TEMPLATES_DIR_NAME).await?;
    let path = template_path(env.as_ref(), name, &directory);
    let (templates, diagnostics) = load_prompt_templates(env.as_ref(), &[path.to_string()]).await;
    if let Some(diagnostic) = diagnostics.first() {
        return Err(AppError::AiHarness(diagnostic.message.clone()));
    }
    let template = templates
        .into_iter()
        .next()
        .ok_or_else(|| AppError::InvalidTemplatePath(path.to_string()))?;
    Ok(TemplateFileOutput {
        name: template.name,
        dir: dir.clone(),
        description: strip_template_file_path(template.description.unwrap_or_default(), &path),
        content: template.content,
    })
}

/// 保存一个模板文件。
/// @param app Tauri 应用句柄。
/// @param input 模板文件保存请求。
pub async fn save_template_file(
    app: &tauri::AppHandle,
    input: SaveTemplateFileInput,
) -> AppResult<()> {
    let env = context::context_async(app).await?.env.read().await.clone();
    validate_template_name(&input.name)?;
    let directory = resource_dir(app, &input.dir, TEMPLATES_DIR_NAME).await?;
    let path = template_path(env.as_ref(), &input.name, &directory);
    let content = render_template_content(&input.description, &path, &input.content)?;
    clear_agent_harness_cache().await?;
    env.write_file(&path, content.as_bytes())
        .await
        .map_err(file_error)?;
    Ok(())
}

/// 删除一个模板文件。
/// @param app Tauri 应用句柄。
/// @param name 模板名，不包含 `.md` 扩展名。
/// @param dir 客户端选择的模板目录类型。
pub async fn delete_template_file(
    app: &tauri::AppHandle,
    name: &str,
    dir: &TemplateDir,
) -> AppResult<()> {
    let env = context::context_async(app).await?.env.read().await.clone();
    validate_template_name(name)?;
    let directory = resource_dir(app, dir, TEMPLATES_DIR_NAME).await?;
    clear_agent_harness_cache().await?;
    env.remove(&template_path(env.as_ref(), name, &directory), false, false)
        .await
        .map_err(file_error)?;
    Ok(())
}

/// 校验 Skill 文件路径位于全局或当前项目 Skill 目录中。
/// @param app Tauri 应用句柄。
/// @param env 当前执行环境。
/// @param path Skill 文件绝对路径。
async fn validate_skill_file_path(
    app: &tauri::AppHandle,
    env: &dyn ExecutionEnv,
    path: &str,
) -> AppResult<()> {
    let info = env.file_info(path).await.map_err(file_error)?;
    if info.kind != FileKind::File {
        return Err(AppError::InvalidSkillPath(path.to_string()));
    }
    for (directory, _) in resource_dirs(app, SKILLS_DIR_NAME).await? {
        let relative = env.relative_path(&directory, path);
        if relative_skill_path(&relative) {
            return Ok(());
        }
    }
    Err(AppError::InvalidSkillPath(path.to_string()))
}

/// 判断路径是否为 Skill 加载器可识别的文件形态。
/// @param relative 相对 `.skills` 目录的路径。
fn relative_skill_path(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.starts_with("../")
        && !relative.starts_with('/')
        && !relative.contains(':')
        && (relative == "SKILL.md"
            || relative.ends_with("/SKILL.md")
            || !relative.contains('/') && relative.ends_with(".md"))
}

/// 通过执行环境精确修改或插入 `disable-model-invocation` frontmatter 字段。
/// @param env 当前执行环境。
/// @param path Skill 文件绝对路径。
/// @param disabled 是否禁止模型自主调用。
async fn update_skill_disable_model_invocation(
    env: &dyn ExecutionEnv,
    path: &str,
    disabled: bool,
) -> AppResult<()> {
    let content = env.read_text_file(path).await.map_err(file_error)?;
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.starts_with("---\n") {
        return Err(AppError::AiHarness(
            "Skill file must start with YAML frontmatter".to_string(),
        ));
    }
    let Some(end_index) = normalized[4..].find("\n---") else {
        return Err(AppError::AiHarness(
            "Skill frontmatter is not closed".to_string(),
        ));
    };
    let end_index = end_index + 4;
    let frontmatter = &normalized[4..end_index];
    let body = &normalized[end_index..];
    let value = if disabled { "true" } else { "false" };
    let mut changed = false;
    let mut lines = frontmatter
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("disable-model-invocation:") {
                changed = true;
                let indent_len = line.len() - trimmed.len();
                format!("{}disable-model-invocation: {value}", &line[..indent_len])
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>();
    if !changed {
        lines.push(format!("disable-model-invocation: {value}"));
    }
    let next_frontmatter = format!("{}\n", lines.join("\n"));
    let next_content = format!("---\n{next_frontmatter}{body}");
    let next_content = if content.contains("\r\n") {
        next_content.replace('\n', "\r\n")
    } else {
        next_content
    };
    env.write_file(path, next_content.as_bytes())
        .await
        .map_err(file_error)?;
    Ok(())
}

/// 清空全部已缓存 AgentHarness。
async fn clear_agent_harness_cache() -> AppResult<()> {
    let agent_harnesses = context::context()
        .agent_harnesses
        .lock()
        .map_err(|error| AppError::ContextLock(error.to_string()))?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for agent_harness in agent_harnesses {
        if !agent_harness.is_idle().await {
            return Err(AppError::AiHarness(
                "清理 Harness 缓存要求全部会话处于空闲状态".to_string(),
            ));
        }
    }
    context::context()
        .agent_harnesses
        .lock()
        .map_err(|error| AppError::ContextLock(error.to_string()))?
        .clear();
    Ok(())
}

/// 校验客户端回传的模板名。
/// @param name 客户端回传的模板名。
fn validate_template_name(name: &str) -> AppResult<()> {
    if name.is_empty() || name.to_ascii_lowercase().ends_with(".md") {
        return Err(AppError::InvalidTemplateName(name.to_string()));
    }
    Ok(())
}

/// 构造模板文件路径并由后端统一追加 `.md` 扩展名。
/// @param env 当前执行环境。
/// @param name 模板名，不包含 `.md` 扩展名。
/// @param dir 模板目录。
fn template_path(env: &dyn ExecutionEnv, name: &str, dir: &str) -> String {
    env.join_path(dir, &format!("{name}.md"))
}

/// 生成符合 PromptTemplate 加载器规则的 Markdown 文件内容。
/// @param description 可选 frontmatter 描述。
/// @param path 模板文件路径。
/// @param content Markdown 模板正文。
fn render_template_content(description: &str, path: &str, content: &str) -> AppResult<String> {
    if description.trim().is_empty() {
        return Ok(content.to_string());
    }
    let frontmatter = serde_yaml::to_string(&TemplateFrontmatter {
        description: format!("{description}\nFile: {path}"),
    })?;
    Ok(format!("---\n{frontmatter}---\n\n{content}"))
}

/// 移除后端自动追加的模板文件路径说明。
/// @param description 加载器解析出的模板描述。
/// @param path 模板文件路径。
fn strip_template_file_path(description: String, path: &str) -> String {
    let suffix = format!("\nFile: {path}");
    description
        .strip_suffix(&suffix)
        .unwrap_or(&description)
        .to_string()
}

/// 仅用于读写模板描述的 YAML frontmatter 结构。
#[derive(Serialize)]
struct TemplateFrontmatter {
    /// 模板的可选描述。
    description: String,
}

/// 将执行环境文件错误保留到应用错误边界。
/// @param error 文件操作错误。
fn file_error(error: FileError) -> AppError {
    AppError::AiHarness(error.to_string())
}
