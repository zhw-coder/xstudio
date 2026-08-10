use ai::{
    agent::{env::ExecutionEnv, AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook},
    model::Tool,
};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{
    lib::{
        edit_diff::{apply_edits, normalize_to_lf, restore_line_endings, strip_bom, Edit},
        file_mutation_queue::FileMutationQueue,
    },
    read::text_result,
};

/// 基于唯一精确文本替换的单文件编辑工具。
#[derive(Debug)]
pub struct EditTool {
    queue: FileMutationQueue,
}

#[async_trait]
impl AgentTool for EditTool {
    /// 创建编辑工具。
    fn new() -> Self {
        Self {
            queue: FileMutationQueue::new(),
        }
    }

    fn name() -> &'static str {
        "edit"
    }

    fn definition(&self) -> Tool {
        Tool {
            name: "edit".to_string(),
            description:
                "Edit a single file using unique, non-overlapping exact text replacements."
                    .to_string(),
            parameters: json!({
                "type":"object", "properties": {
                    "path":{"type":"string","description":"Path to the file to edit (relative or absolute)"},
                    "edits":{"type":"array","minItems":1,"items":{"type":"object","properties":{"oldText":{"type":"string","description":"Unique exact text to replace"},"newText":{"type":"string","description":"Replacement text"}},"required":["oldText","newText"],"additionalProperties":false}}
                }, "required":["path","edits"], "additionalProperties":false
            }),
        }
    }
    fn init(&self, _configs: Value) -> Result<(), AgentToolError> {
        Ok(())
    }
    fn prepare_arguments(&self, mut args: Value) -> Value {
        if let Some(edits) = args
            .get("edits")
            .and_then(Value::as_str)
            .and_then(|value| serde_json::from_str(value).ok())
        {
            args["edits"] = edits;
        }
        if let (Some(old_text), Some(new_text)) =
            (args.get("oldText").cloned(), args.get("newText").cloned())
        {
            let mut edits = args
                .get("edits")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            edits.push(json!({"oldText":old_text,"newText":new_text}));
            args["edits"] = Value::Array(edits);
            if let Some(object) = args.as_object_mut() {
                object.remove("oldText");
                object.remove("newText");
            }
        }
        args
    }
    async fn execute(
        &self,
        env: &dyn ExecutionEnv,
        _tool_call_id: &String,
        params: &Value,
        _on_update: Option<&UpdateToolCallHook>,
    ) -> Result<AgentToolResult, AgentToolError> {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentToolError::Message("path must be a string".to_string()))?;
        let edits: Vec<Edit> = serde_json::from_value(
            params.get("edits").cloned().unwrap_or(Value::Null),
        )
        .map_err(|error| {
            AgentToolError::Message(format!("edits must be a non-empty array: {error}"))
        })?;
        if edits.is_empty() {
            return Err(AgentToolError::Message(
                "edits must contain at least one replacement".to_string(),
            ));
        }
        let resolved_path = env.resolve_path(path);
        self.queue
            .with_file_mutation(&resolved_path, || async {
                let raw = env.read_text_file(path).await.map_err(|error| {
                    AgentToolError::Message(format!("Could not edit file {path}: {error}"))
                })?;
                let (bom, content) = strip_bom(&raw);
                let ending = if content.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let updated = apply_edits(&normalize_to_lf(content), &edits, path)
                    .map_err(AgentToolError::Message)?;
                env.write_file(
                    path,
                    format!("{bom}{}", restore_line_endings(updated, ending)).as_bytes(),
                )
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("Could not write file {path}: {error}"))
                })
            })
            .await?;
        Ok(text_result(
            format!("Successfully replaced {} block(s) in {path}.", edits.len()),
            Value::Null,
        ))
    }
}
