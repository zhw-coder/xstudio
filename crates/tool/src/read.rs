use ai::{
    agent::{
        env::{ExecutionEnv, ReadTextRangeOptions},
        AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook,
    },
    model::{ContentBlock, TextContent, Tool},
};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::lib::truncate::{format_size, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES};

/// 读取文件内容的 Agent 工具。
#[derive(Debug)]
pub struct ReadTool;

#[async_trait]
impl AgentTool for ReadTool {
    /// 创建读取工具。
    fn new() -> Self {
        Self
    }

    fn name() -> &'static str {
        "read"
    }

    fn definition(&self) -> Tool {
        Tool {
            name: "read".to_string(),
            description: format!(
                "Read a text file. Output is truncated to {DEFAULT_MAX_LINES} lines or {}. Use offset and limit for large files.",
                format_size(DEFAULT_MAX_BYTES)
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to read (relative or absolute)" },
                    "offset": { "type": "integer", "minimum": 1, "description": "Line number to start reading from (1-indexed)" },
                    "limit": { "type": "integer", "minimum": 1, "description": "Maximum number of lines to read" }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        }
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
        let path = required_string(params, "path")?;
        let offset = optional_usize(params, "offset")?.unwrap_or(1);
        let limit = optional_usize(params, "limit")?;
        let result = env
            .read_text_range(
                path,
                ReadTextRangeOptions {
                    offset,
                    limit,
                    max_lines: DEFAULT_MAX_LINES,
                    max_bytes: DEFAULT_MAX_BYTES,
                },
            )
            .await
            .map_err(|error| AgentToolError::Message(format!("Failed to read {path}: {error}")))?;
        let first_line_exceeds_limit = result.first_line_exceeds_limit;
        let next_offset = result.next_offset;
        let line_count = result.line_count;
        let mut output = result.content;
        if first_line_exceeds_limit {
            output = format!(
                "[Line {offset} exceeds {} limit. Use bash to inspect it.]",
                format_size(DEFAULT_MAX_BYTES)
            );
        } else if let Some(next_offset) = next_offset {
            output.push_str(&format!(
                "\n\n[Showing lines {offset}-{}. Use offset={next_offset} to continue.]",
                next_offset.saturating_sub(1)
            ));
        }

        Ok(text_result(
            output,
            json!({
                "range": {
                    "lineCount": line_count,
                    "nextOffset": next_offset,
                    "firstLineExceedsLimit": first_line_exceeds_limit,
                }
            }),
        ))
    }
}

/// 从参数对象读取必填字符串。
pub(crate) fn required_string<'a>(
    params: &'a Value,
    name: &str,
) -> Result<&'a str, AgentToolError> {
    params
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| AgentToolError::Message(format!("{name} must be a string")))
}

/// 从参数对象读取可选正整数。
pub(crate) fn optional_usize(params: &Value, name: &str) -> Result<Option<usize>, AgentToolError> {
    match params.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .map(Some)
            .ok_or_else(|| AgentToolError::Message(format!("{name} must be a positive integer"))),
    }
}

/// 创建文本工具结果。
pub(crate) fn text_result(text: String, details: Value) -> AgentToolResult {
    AgentToolResult {
        content: vec![ContentBlock::Text(TextContent {
            text,
            text_signature: None,
        })],
        details,
        terminate: None,
    }
}
