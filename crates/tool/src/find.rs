use ai::{
    agent::{
        env::{ExecutionEnv, FindFilesOptions},
        AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook,
    },
    model::Tool,
};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{
    lib::truncate::{format_size, truncate_head, DEFAULT_MAX_BYTES},
    read::{optional_usize, required_string, text_result},
};

/// 默认查找结果上限。
const DEFAULT_LIMIT: usize = 1_000;

/// 按 glob 查找文件的 Agent 工具。
#[derive(Debug)]
pub struct FindTool;

#[async_trait]
impl AgentTool for FindTool {
    /// 创建文件查找工具。
    fn new() -> Self {
        Self
    }

    fn name() -> &'static str {
        "find"
    }

    fn definition(&self) -> Tool {
        Tool { name: "find".to_string(), description: format!("Find files by glob pattern while respecting .gitignore. Output is truncated to {DEFAULT_LIMIT} results or {}.", format_size(DEFAULT_MAX_BYTES)), parameters: json!({
            "type":"object", "properties": {
                "pattern":{"type":"string","description":"Glob pattern, for example '*.rs' or 'src/**/*.rs'"},
                "path":{"type":"string","description":"Directory to search (default: current directory)"},
                "limit":{"type":"integer","minimum":1,"description":"Maximum number of results (default: 1000)"}
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
        let limit = optional_usize(params, "limit")?.unwrap_or(DEFAULT_LIMIT);
        let root = env
            .real_path(path)
            .await
            .map_err(|error| AgentToolError::Message(format!("Path not found: {path}: {error}")))?;
        let result = env
            .find_files(
                path,
                FindFilesOptions {
                    glob: Some(pattern.to_string()),
                    limit: Some(limit),
                },
            )
            .await
            .map_err(|error| AgentToolError::Message(format!("Failed to search files: {error}")))?;
        let results = result
            .files
            .into_iter()
            .map(|file| {
                std::path::Path::new(&file)
                    .strip_prefix(&root)
                    .unwrap_or(std::path::Path::new(&file))
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect::<Vec<_>>();
        if results.is_empty() {
            return Ok(text_result(
                "No files found matching pattern".to_string(),
                Value::Null,
            ));
        }
        let truncation = truncate_head(&results.join("\n"), usize::MAX, DEFAULT_MAX_BYTES);
        let mut output = truncation.content.clone();
        let mut details = serde_json::Map::new();
        if result.limit_reached {
            output.push_str(&format!(
                "\n\n[{limit} results limit reached. Use limit={} for more, or refine pattern]",
                limit.saturating_mul(2)
            ));
            details.insert("resultLimitReached".to_string(), json!(limit));
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
