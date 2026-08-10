use ai::{
    agent::{env::ExecutionEnv, AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook},
    model::Tool,
};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{
    lib::file_mutation_queue::FileMutationQueue,
    read::{required_string, text_result},
};

/// 覆盖写入文件内容的 Agent 工具。
#[derive(Debug)]
pub struct WriteTool {
    queue: FileMutationQueue,
}

#[async_trait]
impl AgentTool for WriteTool {
    /// 创建写入工具。
    fn new() -> Self {
        Self {
            queue: FileMutationQueue::new(),
        }
    }

    fn name() -> &'static str {
        "write"
    }

    fn definition(&self) -> Tool {
        Tool {
            name: "write".to_string(),
            description:
                "Write content to a file. Creates parent directories and overwrites existing files."
                    .to_string(),
            parameters: json!({
                "type":"object", "properties": {
                    "path":{"type":"string","description":"Path to the file to write (relative or absolute)"},
                    "content":{"type":"string","description":"Content to write to the file"}
                }, "required":["path","content"], "additionalProperties":false
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
        let content = required_string(params, "content")?;
        let resolved_path = env.resolve_path(path);
        self.queue
            .with_file_mutation(&resolved_path, || async {
                env.write_file(path, content.as_bytes())
                    .await
                    .map_err(|error| {
                        AgentToolError::Message(format!("Failed to write {path}: {error}"))
                    })
            })
            .await?;
        Ok(text_result(
            format!("Successfully wrote {} bytes to {path}", content.len()),
            Value::Null,
        ))
    }
}
