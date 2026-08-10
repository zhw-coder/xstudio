//! 从执行环境目录递归加载 `SKILL.md` / 根目录 markdown Skill，解析 YAML frontmatter，执行基础校验，
//! 并提供显式调用 Skill 的 prompt 渲染函数。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::agent::{
    env::{ExecutionEnv, FileInfo, FileKind},
    harness::types::Skill,
};

/// Skill 名称允许的最大字符数。
const MAX_NAME_LENGTH: usize = 64;
/// Skill 描述允许的最大字符数。
const MAX_DESCRIPTION_LENGTH: usize = 1024;
/// ignore 文件名列表。
const IGNORE_FILE_NAMES: [&str; 3] = [".gitignore", ".ignore", ".fdignore"];

/// 加载 Skill 时产生的诊断信息。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillDiagnostic {
    /// 诊断严重级别；当前实现仅产生 `warning`。
    #[serde(rename = "type")]
    pub kind: String,
    /// 面向人类阅读的诊断消息文本。
    pub message: String,
    /// 与该诊断相关联的文件或目录路径。
    pub path: String,
}

/// 带来源标签的 Skill 加载结果。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourcedSkill<TSource> {
    /// 已加载 Skill。
    pub skill: Skill,
    /// 来源标签。
    pub source: TSource,
}

/// 带来源标签的诊断信息。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourcedSkillDiagnostic<TSource> {
    /// 诊断信息。
    pub diagnostic: SkillDiagnostic,
    /// 来源标签。
    pub source: TSource,
}

/// 渲染一次 Skill 显式调用的 prompt 文本。
pub fn format_skill_invocation(env: &dyn ExecutionEnv, skill: &Skill, additional_instructions: Option<&str>) -> String {
    let skill_block = format!(
        "<skill name=\"{}\" location=\"{}\">\nReferences are relative to {}.\n\n{}\n</skill>",
        skill.name,
        skill.file_path,
        env.dirname_path(&skill.file_path),
        skill.content
    );
    match additional_instructions.filter(|value| !value.is_empty()) {
        Some(instructions) => format!("{skill_block}\n\n{instructions}"),
        None => skill_block,
    }
}

/// 把可供模型自主选择的 Skill 列表格式化为系统提示词片段。
///
/// @param skills 已加载的 Skill 列表。
pub fn format_skills_for_system_prompt(skills: &[Skill]) -> String {
    let visible: Vec<&Skill> = skills.iter().filter(|skill| !skill.disable_model_invocation).collect();
    if visible.is_empty() {
        return String::new();
    }
    let mut output = String::from(
        "The following skills provide specialized instructions for specific tasks.\n\
Read the full skill file when the task matches its description.\n\
When a skill file references a relative path, resolve it against the skill directory \
(parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.\n\n\
<available_skills>\n",
    );
    for skill in visible {
        output.push_str("  <skill>\n");
        output.push_str(&format!("    <name>{}</name>\n", escape_xml(&skill.name)));
        output.push_str(&format!("    <description>{}</description>\n", escape_xml(&skill.description)));
        output.push_str(&format!("    <location>{}</location>\n", escape_xml(&skill.file_path)));
        output.push_str("  </skill>\n");
    }
    output.push_str("</available_skills>");
    output
}

/// 从一个或多个目录加载 Skill。
pub async fn load_skills<E>(env: &E, dirs: &[String]) -> (Vec<Skill>, Vec<SkillDiagnostic>)
where
    E: ExecutionEnv + ?Sized,
{
    let mut skills = Vec::new();
    let mut diagnostics = Vec::new();
    for dir in dirs {
        let Some(root_info) = safe_file_info(env, dir).await else { continue };
        if resolve_kind(env, &root_info).await.as_deref() != Some("directory") {
            continue;
        }
        let mut ignore_patterns = Vec::new();
        let (loaded, warnings) =
            load_skills_from_dir_internal(env, &root_info.path, true, &mut ignore_patterns, &root_info.path).await;
        skills.extend(loaded);
        diagnostics.extend(warnings);
    }
    (skills, diagnostics)
}

/// 从带来源标签的目录数组加载 Skill。
pub async fn load_sourced_skills<E, TSource>(
    env: &E,
    inputs: &[(String, TSource)],
) -> (Vec<SourcedSkill<TSource>>, Vec<SourcedSkillDiagnostic<TSource>>)
where
    E: ExecutionEnv + ?Sized,
    TSource: Clone,
{
    let mut skills = Vec::new();
    let mut diagnostics = Vec::new();
    for (path, source) in inputs {
        let (loaded, warnings) = load_skills(env, std::slice::from_ref(path)).await;
        skills.extend(loaded.into_iter().map(|skill| SourcedSkill { skill, source: source.clone() }));
        diagnostics.extend(
            warnings.into_iter().map(|diagnostic| SourcedSkillDiagnostic { diagnostic, source: source.clone() }),
        );
    }
    (skills, diagnostics)
}

/// 递归加载某个目录下的全部 Skill。
async fn load_skills_from_dir_internal<E>(
    env: &E,
    dir: &str,
    include_root_files: bool,
    ignore_patterns: &mut Vec<String>,
    root_dir: &str,
) -> (Vec<Skill>, Vec<SkillDiagnostic>)
where
    E: ExecutionEnv + ?Sized,
{
    let mut skills = Vec::new();
    let mut diagnostics = Vec::new();
    if !env.exists(dir).await.unwrap_or(false) {
        return (skills, diagnostics);
    }
    let Some(dir_info) = safe_file_info(env, dir).await else { return (skills, diagnostics) };
    if resolve_kind(env, &dir_info).await.as_deref() != Some("directory") {
        return (skills, diagnostics);
    }
    add_ignore_rules(env, ignore_patterns, dir, root_dir).await;
    let mut entries = match env.list_dir(dir).await {
        Ok(entries) => entries,
        Err(_) => return (skills, diagnostics),
    };
    for entry in &entries {
        if entry.name != "SKILL.md" {
            continue;
        }
        if resolve_kind(env, entry).await.as_deref() != Some("file") {
            continue;
        }
        let rel_path = env.relative_path(root_dir, &entry.path);
        if is_ignored(&rel_path, ignore_patterns) {
            continue;
        }
        let (skill, warnings) = load_skill_from_file(env, &entry.path).await;
        if let Some(skill) = skill {
            skills.push(skill);
        }
        diagnostics.extend(warnings);
        return (skills, diagnostics);
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in entries {
        if entry.name.starts_with('.') || entry.name == "node_modules" {
            continue;
        }
        let Some(kind) = resolve_kind(env, &entry).await else { continue };
        let rel_path = env.relative_path(root_dir, &entry.path);
        let ignore_path = if kind == "directory" { format!("{rel_path}/") } else { rel_path };
        if is_ignored(&ignore_path, ignore_patterns) {
            continue;
        }
        if kind == "directory" {
            let (loaded, warnings) =
                Box::pin(load_skills_from_dir_internal(env, &entry.path, false, ignore_patterns, root_dir)).await;
            skills.extend(loaded);
            diagnostics.extend(warnings);
        } else if kind == "file" && include_root_files && entry.name.ends_with(".md") {
            let (skill, warnings) = load_skill_from_file(env, &entry.path).await;
            if let Some(skill) = skill {
                skills.push(skill);
            }
            diagnostics.extend(warnings);
        }
    }
    (skills, diagnostics)
}

/// 把当前目录下的 ignore 文件追加到累积模式中。
async fn add_ignore_rules<E>(env: &E, patterns: &mut Vec<String>, dir: &str, root_dir: &str)
where
    E: ExecutionEnv + ?Sized,
{
    let relative_dir = env.relative_path(root_dir, dir);
    let prefix = if relative_dir.is_empty() { String::new() } else { format!("{relative_dir}/") };
    for filename in IGNORE_FILE_NAMES {
        let ignore_path = env.join_path(dir, filename);
        let Some(info) = safe_file_info(env, &ignore_path).await else { continue };
        if info.kind != FileKind::File {
            continue;
        }
        let Ok(content) = env.read_text_file(&ignore_path).await else { continue };
        patterns.extend(content.lines().filter_map(|line| prefix_ignore_pattern(line, &prefix)));
    }
}

/// 把单条 ignore 规则按递归层级加上路径前缀。
fn prefix_ignore_pattern(line: &str, prefix: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.starts_with("\\#")) {
        return None;
    }
    let mut pattern = line.to_string();
    let mut negated = false;
    if pattern.starts_with('!') {
        negated = true;
        pattern.remove(0);
    } else if pattern.starts_with("\\!") {
        pattern.remove(0);
    }
    if pattern.starts_with('/') {
        pattern.remove(0);
    }
    let prefixed = if prefix.is_empty() { pattern } else { format!("{prefix}{pattern}") };
    Some(if negated { format!("!{prefixed}") } else { prefixed })
}

/// 从单个 Skill 文件加载 Skill。
async fn load_skill_from_file<E>(env: &E, file_path: &str) -> (Option<Skill>, Vec<SkillDiagnostic>)
where
    E: ExecutionEnv + ?Sized,
{
    let mut diagnostics = Vec::new();
    let raw_content = match env.read_text_file(file_path).await {
        Ok(content) => content,
        Err(error) => return (None, vec![warning(file_path, &error.to_string())]),
    };
    let (frontmatter, body) = parse_frontmatter(&raw_content);
    let skill_dir = env.dirname_path(file_path);
    let parent_dir_name = env.basename_path(&skill_dir);
    let description = frontmatter.get("description").and_then(Value::as_str).map(str::to_string);
    for error in validate_description(description.as_deref()) {
        diagnostics.push(warning(file_path, &error));
    }
    let name = frontmatter.get("name").and_then(Value::as_str).unwrap_or(&parent_dir_name).to_string();
    for error in validate_name(&name, &parent_dir_name) {
        diagnostics.push(warning(file_path, &error));
    }
    let Some(description) = description.filter(|value| !value.trim().is_empty()) else {
        return (None, diagnostics);
    };
    let disable_model_invocation =
        frontmatter.get("disable-model-invocation").and_then(Value::as_bool).unwrap_or(false);
    (
        Some(Skill {
            name,
            description,
            content: body,
            file_path: file_path.to_string(),
            disable_model_invocation,
        }),
        diagnostics,
    )
}

/// 校验 Skill 名称。
fn validate_name(name: &str, parent_dir_name: &str) -> Vec<String> {
    let mut errors = Vec::new();
    if name != parent_dir_name {
        errors.push(format!("name \"{name}\" does not match parent directory \"{parent_dir_name}\""));
    }
    if name.len() > MAX_NAME_LENGTH {
        errors.push(format!("name exceeds {MAX_NAME_LENGTH} characters ({})", name.len()));
    }
    if !name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
        errors.push("name contains invalid characters (must be lowercase a-z, 0-9, hyphens only)".to_string());
    }
    if name.starts_with('-') || name.ends_with('-') {
        errors.push("name must not start or end with a hyphen".to_string());
    }
    if name.contains("--") {
        errors.push("name must not contain consecutive hyphens".to_string());
    }
    errors
}

/// 校验 Skill 描述。
fn validate_description(description: Option<&str>) -> Vec<String> {
    match description {
        None => vec!["description is required".to_string()],
        Some(value) if value.trim().is_empty() => vec!["description is required".to_string()],
        Some(value) if value.len() > MAX_DESCRIPTION_LENGTH => {
            vec![format!("description exceeds {MAX_DESCRIPTION_LENGTH} characters ({})", value.len())]
        }
        Some(_) => Vec::new(),
    }
}

/// 解析 markdown frontmatter。
fn parse_frontmatter(content: &str) -> (HashMap<String, Value>, String) {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    if !normalized.starts_with("---") {
        return (HashMap::new(), normalized);
    }
    let Some(end_index) = normalized[3..].find("\n---") else {
        return (HashMap::new(), normalized);
    };
    let end_index = end_index + 3;
    let yaml_string = &normalized[4..end_index];
    let body = normalized[end_index + 4..].trim().to_string();
    let frontmatter = serde_yaml::from_str::<HashMap<String, Value>>(yaml_string).unwrap_or_default();
    (frontmatter, body)
}

/// 安全获取路径元信息。
async fn safe_file_info<E>(env: &E, path: &str) -> Option<FileInfo>
where
    E: ExecutionEnv + ?Sized,
{
    env.file_info(path).await.ok()
}

/// 解析 `FileInfo` 的有效类型。
async fn resolve_kind<E>(env: &E, info: &FileInfo) -> Option<String>
where
    E: ExecutionEnv + ?Sized,
{
    match info.kind {
        FileKind::File => Some("file".to_string()),
        FileKind::Directory => Some("directory".to_string()),
        FileKind::Symlink => {
            let real_path = env.real_path(&info.path).await.ok()?;
            let target = env.file_info(&real_path).await.ok()?;
            match target.kind {
                FileKind::File => Some("file".to_string()),
                FileKind::Directory => Some("directory".to_string()),
                FileKind::Symlink => None,
            }
        }
    }
}

/// 简化 ignore 匹配；支持常见前缀、目录与通配后缀场景。
fn is_ignored(path: &str, patterns: &[String]) -> bool {
    let mut ignored = false;
    for pattern in patterns {
        let (negated, pattern) = pattern.strip_prefix('!').map_or((false, pattern.as_str()), |rest| (true, rest));
        if simple_pattern_matches(path, pattern) {
            ignored = !negated;
        }
    }
    ignored
}

/// 简化模式匹配。
fn simple_pattern_matches(path: &str, pattern: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    path == pattern
        || path.starts_with(&format!("{pattern}/"))
        || pattern.strip_prefix("*").is_some_and(|suffix| path.ends_with(suffix))
}

/// 构造 warning 诊断。
fn warning(path: &str, message: &str) -> SkillDiagnostic {
    SkillDiagnostic { kind: "warning".to_string(), message: message.to_string(), path: path.to_string() }
}

/// 转义系统提示词 XML 片段中的文本内容。
///
/// @param value 待转义的文本。
fn escape_xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}
