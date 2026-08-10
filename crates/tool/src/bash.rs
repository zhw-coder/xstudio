use ai::{
    agent::{
        env::{ExecutionEnv, ExecutionEnvExecOptions},
        AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook,
    },
    model::Tool,
};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{
    lib::truncate::{format_size, truncate_head, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES},
    read::{optional_usize, required_string, text_result},
};

/// 在指定工作目录执行 shell 命令的 Agent 工具。
#[derive(Debug)]
pub struct BashTool {
    command_prefix: Option<String>,
}

impl BashTool {
    /// 设置每个命令前插入的 shell 初始化文本。
    pub fn with_command_prefix(mut self, command_prefix: impl Into<String>) -> Self {
        self.command_prefix = Some(command_prefix.into());
        self
    }
}

#[async_trait]
impl AgentTool for BashTool {
    /// 创建使用当前系统 shell 的命令工具。
    fn new() -> Self {
        Self {
            command_prefix: None,
        }
    }

    fn name() -> &'static str {
        "bash"
    }

    fn definition(&self) -> Tool {
        Tool { name: "bash".to_string(), description: format!("Execute a shell command in the working directory. Output is truncated to {DEFAULT_MAX_LINES} lines or {}. Optionally provide timeout in seconds.", format_size(DEFAULT_MAX_BYTES)), parameters: json!({
            "type":"object", "properties": {
                "command":{"type":"string","description":"Shell command to execute"},
                "timeout":{"type":"integer","minimum":1,"description":"Timeout in seconds"}
            }, "required":["command"], "additionalProperties":false
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
        on_update: Option<&UpdateToolCallHook>,
    ) -> Result<AgentToolResult, AgentToolError> {
        let command = required_string(params, "command")?;
        let timeout = optional_usize(params, "timeout")?;
        let command = self.command_prefix.as_ref().map_or_else(
            || command.to_string(),
            |prefix| format!("{prefix}\n{command}"),
        );
        let execution = env
            .exec(
                &command,
                Some(ExecutionEnvExecOptions {
                    timeout: timeout.map(|value| value as u64),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|error| {
                AgentToolError::Message(format!("Failed to execute shell: {error}"))
            })?;
        let output = format!("{}{}", execution.stdout, execution.stderr);
        let truncation = truncate_head(&output, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
        if let Some(callback) = on_update {
            callback(text_result(
                truncation.content.clone(),
                json!({"truncation": truncation}),
            ));
        }
        let mut text = truncation.content.clone();
        if execution.exit_code != 0 {
            text.push_str(&format!(
                "\n\nCommand exited with code {}",
                execution.exit_code
            ));
            return Err(AgentToolError::Message(text));
        }
        Ok(text_result(
            if text.is_empty() {
                "(no output)".to_string()
            } else {
                text
            },
            json!({"truncation": truncation}),
        ))
    }
}
