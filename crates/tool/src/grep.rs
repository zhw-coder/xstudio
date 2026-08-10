use futures::stream::{self, StreamExt};
use std::path::Path;

use ai::{
    agent::{
        env::{ExecutionEnv, FindFilesOptions},
        AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook,
    },
    model::Tool,
};
use async_trait::async_trait;
use regex::{Regex, RegexBuilder};
use serde_json::{json, Value};

use crate::{
    lib::truncate::{format_size, truncate_head, truncate_line, DEFAULT_MAX_BYTES},
    read::{optional_usize, required_string, text_result},
};

/// 默认匹配数上限。
const DEFAULT_LIMIT: usize = 100;

/// 基于正则表达式搜索文件内容的 Agent 工具。
#[derive(Debug)]
pub struct GrepTool;

#[async_trait]
impl AgentTool for GrepTool {
    /// 创建内容搜索工具。
    fn new() -> Self {
        Self
    }

    fn name() -> &'static str {
        "grep"
    }

    fn definition(&self) -> Tool {
        Tool { name: "grep".to_string(), description: format!("Search file contents for a regex or literal pattern while respecting .gitignore. Output is truncated to {DEFAULT_LIMIT} matches or {}.", format_size(DEFAULT_MAX_BYTES)), parameters: json!({
            "type":"object", "properties": {
                "pattern":{"type":"string","description":"Search pattern (regex or literal string)"},
                "path":{"type":"string","description":"Directory or file to search (default: current directory)"},
                "glob":{"type":"string","description":"Filter files by glob pattern"},
                "ignoreCase":{"type":"boolean","description":"Case-insensitive search"},
                "literal":{"type":"boolean","description":"Treat pattern as a literal string"},
                "context":{"type":"integer","minimum":0,"description":"Lines to show before and after each match"},
                "limit":{"type":"integer","minimum":1,"description":"Maximum matches to return (default: 100)"}
            }, "required":["pattern"], "additionalProperties":false
        }) }
    }
    fn init(&self, _configs: Value) -> Result<(), AgentToolError> {
        Ok(())
    }
    async fn execute(
        &self,
        env: &dyn ExecutionEnv,
        _tool_call_id: &String,
        params: &Value,
        _on_update: Option<&UpdateToolCallHook>,
    ) -> Result<AgentToolResult, AgentToolError> {
        let pattern = required_string(params, "pattern")?;
        let path = params.get("path").and_then(Value::as_str).unwrap_or(".");
        let root = env
            .real_path(path)
            .await
            .map_err(|error| AgentToolError::Message(format!("Path not found: {path}: {error}")))?;
        let limit = optional_usize(params, "limit")?.unwrap_or(DEFAULT_LIMIT);
        let context = params.get("context").and_then(Value::as_u64).unwrap_or(0) as usize;
        let expression = if params
            .get("literal")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };
        let regex = RegexBuilder::new(&expression)
            .case_insensitive(
                params
                    .get("ignoreCase")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
            .build()
            .map_err(|error| AgentToolError::Message(format!("Invalid search pattern: {error}")))?;
        let files = env
            .find_files(
                path,
                FindFilesOptions {
                    glob: params
                        .get("glob")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    limit: None,
                },
            )
            .await
            .map_err(|error| AgentToolError::Message(format!("Failed to search files: {error}")))?
            .files;
        let mut searches = stream::iter(files.into_iter().map(|file| {
            let root = root.clone();
            let regex = regex.clone();
            async move { search_file(env, &file, Path::new(&root), &regex, context).await }
        }))
        .buffer_unordered(16);
        let mut matches = Vec::new();
        while let Some(result) = searches.next().await {
            if let Ok(lines) = result {
                for line in lines {
                    matches.push(line);
                    if matches.len() >= limit {
                        break;
                    }
                }
            }
            if matches.len() >= limit {
                break;
            }
        }
        if matches.is_empty() {
            return Ok(text_result("No matches found".to_string(), Value::Null));
        }
        let match_limit_reached = matches.len() >= limit;
        let truncation = truncate_head(&matches.join("\n"), usize::MAX, DEFAULT_MAX_BYTES);
        let mut output = truncation.content.clone();
        let mut details = serde_json::Map::new();
        if match_limit_reached {
            output.push_str(&format!(
                "\n\n[{limit} matches limit reached. Use limit={} for more, or refine pattern]",
                limit.saturating_mul(2)
            ));
            details.insert("matchLimitReached".to_string(), json!(limit));
        }
        if truncation.truncated {
            output.push_str(&format!(
                "\n\n[{} limit reached]",
                format_size(DEFAULT_MAX_BYTES)
            ));
            details.insert("truncation".to_string(), json!(truncation));
        }
        Ok(text_result(output, Value::Object(details)))
    }
}

/// 从一个 UTF-8 文本文件提取匹配行与所需上下文。
async fn search_file(
    env: &dyn ExecutionEnv,
    path: &str,
    root: &Path,
    regex: &Regex,
    context: usize,
) -> Result<Vec<String>, AgentToolError> {
    let content = env
        .read_text_file(path)
        .await
        .map_err(|error| AgentToolError::Message(error.to_string()))?;
    let lines: Vec<&str> = content.lines().collect();
    let path = Path::new(path);
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    let mut output = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !regex.is_match(line) {
            continue;
        }
        let start = index.saturating_sub(context);
        let end = (index + context + 1).min(lines.len());
        for current in start..end {
            let (text, _) = truncate_line(lines[current]);
            let separator = if current == index { ':' } else { '-' };
            output.push(format!(
                "{relative}{separator}{}{} {text}",
                current + 1,
                if current == index { ":" } else { "-" }
            ));
        }
    }
    Ok(output)
}
