//! 从 `ExecutionEnv` 指向的目录或 markdown 文件加载提示词模板，解析 YAML frontmatter，并提供模板
//! 参数替换工具。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::agent::{
    env::{ExecutionEnv, FileInfo, FileKind},
    harness::types::PromptTemplate,
};

/// 加载提示词模板时产生的诊断信息。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptTemplateDiagnostic {
    /// 诊断严重级别；当前实现仅产生 `warning`。
    #[serde(rename = "type")]
    pub kind: String,
    /// 面向人类阅读的诊断消息文本。
    pub message: String,
    /// 与该诊断相关联的文件或目录路径。
    pub path: String,
}

/// 带来源标签的提示词模板加载结果。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourcedPromptTemplate<TSource> {
    /// 已加载模板。
    pub prompt_template: PromptTemplate,
    /// 来源标签。
    pub source: TSource,
}

/// 带来源标签的诊断信息。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourcedPromptTemplateDiagnostic<TSource> {
    /// 诊断信息。
    pub diagnostic: PromptTemplateDiagnostic,
    /// 来源标签。
    pub source: TSource,
}

/// 从一个或多个路径加载提示词模板。
pub async fn load_prompt_templates<E>(env: &E, paths: &[String]) -> (Vec<PromptTemplate>, Vec<PromptTemplateDiagnostic>)
where
    E: ExecutionEnv + ?Sized,
{
    let mut prompt_templates = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        let Some(info) = safe_file_info(env, path).await else { continue };
        match resolve_kind(env, &info).await.as_deref() {
            Some("directory") => {
                let (templates, warnings) = load_templates_from_dir(env, &info.path).await;
                prompt_templates.extend(templates);
                diagnostics.extend(warnings);
            }
            Some("file") if info.name.ends_with(".md") => {
                let (template, warnings) = load_template_from_file(env, &info.path).await;
                if let Some(template) = template {
                    prompt_templates.push(template);
                }
                diagnostics.extend(warnings);
            }
            _ => {}
        }
    }
    (prompt_templates, diagnostics)
}

/// 从带来源标签的路径数组加载提示词模板。
pub async fn load_sourced_prompt_templates<E, TSource>(
    env: &E,
    inputs: &[(String, TSource)],
) -> (Vec<SourcedPromptTemplate<TSource>>, Vec<SourcedPromptTemplateDiagnostic<TSource>>)
where
    E: ExecutionEnv + ?Sized,
    TSource: Clone,
{
    let mut prompt_templates = Vec::new();
    let mut diagnostics = Vec::new();
    for (path, source) in inputs {
        let (templates, warnings) = load_prompt_templates(env, std::slice::from_ref(path)).await;
        prompt_templates.extend(
            templates
                .into_iter()
                .map(|prompt_template| SourcedPromptTemplate { prompt_template, source: source.clone() }),
        );
        diagnostics.extend(
            warnings
                .into_iter()
                .map(|diagnostic| SourcedPromptTemplateDiagnostic { diagnostic, source: source.clone() }),
        );
    }
    (prompt_templates, diagnostics)
}

/// 加载一个目录下所有直接 `.md` 提示词模板。
async fn load_templates_from_dir<E>(env: &E, dir: &str) -> (Vec<PromptTemplate>, Vec<PromptTemplateDiagnostic>)
where
    E: ExecutionEnv + ?Sized,
{
    let mut prompt_templates = Vec::new();
    let mut diagnostics = Vec::new();
    let mut entries = match env.list_dir(dir).await {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(warning(dir, &error.to_string()));
            return (prompt_templates, diagnostics);
        }
    };
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in entries {
        if resolve_kind(env, &entry).await.as_deref() != Some("file") || !entry.name.ends_with(".md") {
            continue;
        }
        let (template, warnings) = load_template_from_file(env, &entry.path).await;
        if let Some(template) = template {
            prompt_templates.push(template);
        }
        diagnostics.extend(warnings);
    }
    (prompt_templates, diagnostics)
}

/// 从单个 `.md` 文件加载提示词模板。
async fn load_template_from_file<E>(env: &E, file_path: &str) -> (Option<PromptTemplate>, Vec<PromptTemplateDiagnostic>)
where
    E: ExecutionEnv + ?Sized,
{
    match env.read_text_file(file_path).await {
        Ok(raw_content) => {
            let (frontmatter, body) = parse_frontmatter(&raw_content);
            let first_line = body.lines().find(|line| !line.trim().is_empty());
            let mut description =
                frontmatter.get("description").and_then(Value::as_str).unwrap_or_default().to_string();
            if description.is_empty() {
                if let Some(first_line) = first_line {
                    description = first_line.chars().take(60).collect();
                    if first_line.chars().count() > 60 {
                        description.push_str("...");
                    }
                }
            }
            (
                Some(PromptTemplate {
                    name: env.basename_path(file_path).trim_end_matches(".md").to_string(),
                    description: Some(description),
                    content: body,
                }),
                Vec::new(),
            )
        }
        Err(error) => (None, vec![warning(file_path, &error.to_string())]),
    }
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

/// 用最简化 shell 风格规则把参数字符串拆分为参数数组。
pub fn parse_command_args(args_string: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    for ch in args_string.chars() {
        if let Some(quote) = in_quote {
            if ch == quote {
                in_quote = None;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quote = Some(ch);
        } else if ch == ' ' || ch == '\t' {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

/// 将提示词模板中的占位符替换为命令参数。
pub fn substitute_args(content: &str, args: &[String]) -> String {
    let mut result = content.to_string();
    for (index, arg) in args.iter().enumerate() {
        result = result.replace(&format!("${}", index + 1), arg);
    }
    let all_args = args.join(" ");
    result = replace_slice_placeholders(&result, args);
    result.replace("$ARGUMENTS", &all_args).replace("$@", &all_args)
}

/// 把一次提示词模板调用渲染为最终 prompt 字符串。
pub fn format_prompt_template_invocation(template: &PromptTemplate, args: &[String]) -> String {
    substitute_args(&template.content, args)
}

/// 替换 `${@:N}` 与 `${@:N:L}` 形式的切片占位符。
fn replace_slice_placeholders(content: &str, args: &[String]) -> String {
    let mut output = String::new();
    let mut rest = content;
    while let Some(start) = rest.find("${@:") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let Some(end) = after.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let spec = &after[..end];
        let mut parts = spec.split(':');
        let start_index = parts.next().and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).saturating_sub(1);
        let rendered = if let Some(length) = parts.next().and_then(|v| v.parse::<usize>().ok()) {
            args.iter().skip(start_index).take(length).cloned().collect::<Vec<_>>().join(" ")
        } else {
            args.iter().skip(start_index).cloned().collect::<Vec<_>>().join(" ")
        };
        output.push_str(&rendered);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

/// 构造 warning 诊断。
fn warning(path: &str, message: &str) -> PromptTemplateDiagnostic {
    PromptTemplateDiagnostic { kind: "warning".to_string(), message: message.to_string(), path: path.to_string() }
}
