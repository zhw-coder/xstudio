use ai::{
    agent::{
        env::{ExecutionEnv, FileKind},
        AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook,
    },
    model::Tool,
};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{
    lib::truncate::{format_size, truncate_head, DEFAULT_MAX_BYTES},
    read::{optional_usize, text_result},
};

/// 默认目录条目上限。
const DEFAULT_LIMIT: usize = 500;

/// 列出目录内容的 Agent 工具。
#[derive(Debug)]
pub struct LsTool;

#[async_trait]
impl AgentTool for LsTool {
    /// 创建目录工具。
    fn new() -> Self {
        Self
    }

    fn name() -> &'static str {
        "ls"
    }

    fn definition(&self) -> Tool {
        Tool { name: "ls".to_string(), description: format!("List directory contents. Includes dotfiles. Output is truncated to {DEFAULT_LIMIT} entries or {}.", format_size(DEFAULT_MAX_BYTES)), parameters: json!({
            "type":"object", "properties": {
                "path":{"type":"string","description":"Directory to list (default: current directory)"},
                "limit":{"type":"integer","minimum":1,"description":"Maximum number of entries to return (default: 500)"}
            }, "additionalProperties":false
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
        let path = params.get("path").and_then(Value::as_str).unwrap_or(".");
        let limit = optional_usize(params, "limit")?.unwrap_or(DEFAULT_LIMIT);
        let entries = env.list_dir(path).await.map_err(|error| {
            AgentToolError::Message(format!("Cannot read directory {path}: {error}"))
        })?;
        let mut names = Vec::new();
        for entry in entries {
            let mut name = entry.name;
            if entry.kind == FileKind::Directory {
                name.push('/');
            }
            names.push(name);
        }
        names.sort_by_key(|name| name.to_lowercase());
        let entry_limit_reached = names.len() > limit;
        names.truncate(limit);
        if names.is_empty() {
            return Ok(text_result("(empty directory)".to_string(), Value::Null));
        }
        let truncation = truncate_head(&names.join("\n"), usize::MAX, DEFAULT_MAX_BYTES);
        let mut output = truncation.content.clone();
        let mut details = serde_json::Map::new();
        if entry_limit_reached {
            output.push_str(&format!(
                "\n\n[{limit} entries limit reached. Use limit={} for more]",
                limit.saturating_mul(2)
            ));
            details.insert("entryLimitReached".to_string(), json!(limit));
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
