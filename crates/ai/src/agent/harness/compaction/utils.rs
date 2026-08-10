//! 上下文压缩（Compaction）与分支总结（Branch Summary）共享的纯工具函数集合。

use std::{borrow::Cow, collections::HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    agent::{
        harness::messages::{convert_to_llm, HarnessMessage},
        types::AgentMessage,
    },
    model::types::{ContentBlock, Message, UserContent},
};

/// 序列化对话时单条 tool result 文本最大保留字符数。
pub const TOOL_RESULT_MAX_CHARS: usize = 2000;

/// 上下文压缩与分支总结统一使用的系统提示词。
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI coding assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";

/// 在一段对话中被工具调用读到、写到、编辑过的文件路径集合。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileOperations {
    /// 被 `read` 工具调用读取过的文件绝对路径集合。
    pub read: HashSet<String>,
    /// 被 `write` 工具调用整体写入过的文件绝对路径集合。
    pub written: HashSet<String>,
    /// 被 `edit` 工具调用编辑过的文件绝对路径集合。
    pub edited: HashSet<String>,
}

/// 创建一个空的 `FileOperations` 累加器。
pub fn create_file_ops() -> FileOperations {
    FileOperations::default()
}

/// 从一条 assistant 消息中识别 read/write/edit 工具调用。
pub fn extract_file_ops_from_message(message: &AgentMessage, file_ops: &mut FileOperations) {
    let AgentMessage::Assistant(message) = message else { return };
    for block in &message.content {
        let ContentBlock::ToolCall(tool_call) = block else { continue };
        let path = tool_call.arguments.get("path").and_then(Value::as_str);
        let Some(path) = path else { continue };
        match tool_call.name.as_str() {
            "read" => {
                file_ops.read.insert(path.to_string());
            }
            "write" => {
                file_ops.written.insert(path.to_string());
            }
            "edit" => {
                file_ops.edited.insert(path.to_string());
            }
            _ => {}
        }
    }
}

/// 把 `FileOperations` 折算为最终的“只读文件 / 修改文件”两份清单。
pub fn compute_file_lists(file_ops: &FileOperations) -> FileLists {
    let modified = file_ops.edited.union(&file_ops.written).cloned().collect::<HashSet<_>>();
    let mut read_files = file_ops.read.iter().filter(|path| !modified.contains(*path)).cloned().collect::<Vec<_>>();
    let mut modified_files = modified.into_iter().collect::<Vec<_>>();
    read_files.sort();
    modified_files.sort();
    FileLists { read_files, modified_files }
}

/// 折算后的文件列表。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileLists {
    /// 仅读未改文件路径列表。
    pub read_files: Vec<String>,
    /// 被写入或编辑文件路径列表。
    pub modified_files: Vec<String>,
}

/// 把“只读文件 / 修改文件”两份清单格式化为 XML 块。
pub fn format_file_operations(read_files: &[String], modified_files: &[String]) -> String {
    let mut sections = Vec::new();
    if !read_files.is_empty() {
        sections.push(format!("<read-files>\n{}\n</read-files>", read_files.join("\n")));
    }
    if !modified_files.is_empty() {
        sections.push(format!("<modified-files>\n{}\n</modified-files>", modified_files.join("\n")));
    }
    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", sections.join("\n\n"))
    }
}

/// 把过长的 tool result 文本按字符数截断。
fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated = text.chars().take(max_chars).collect::<String>();
    let truncated_chars = text.chars().count().saturating_sub(max_chars);
    format!("{truncated}\n\n[... {truncated_chars} more characters truncated]")
}

/// 把 LLM `Message[]` 渲染成单段对话纯文本。
pub fn serialize_conversation(messages: &[Message]) -> String {
    let mut parts = Vec::new();
    for message in messages {
        match message {
            Message::User(message) => {
                let content = match &message.content {
                    UserContent::Text(text) => Cow::Borrowed(text.as_str()),
                    UserContent::Blocks(blocks) => blocks
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text(text) => Some(text.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("")
                        .into(),
                };
                if !content.is_empty() {
                    parts.push(format!("[User]: {content}"));
                }
            }
            Message::Assistant(message) => {
                let mut text_parts = Vec::new();
                let mut thinking_parts = Vec::new();
                let mut tool_calls = Vec::new();
                for block in &message.content {
                    match block {
                        ContentBlock::Text(text) => text_parts.push(text.text.as_str()),
                        ContentBlock::Thinking(thinking) => thinking_parts.push(thinking.thinking.as_str()),
                        ContentBlock::ToolCall(tool_call) => {
                            let args = tool_call
                                .arguments
                                .iter()
                                .map(|(key, value)| format!("{key}={value}"))
                                .collect::<Vec<_>>()
                                .join(", ");
                            tool_calls.push(format!("{}({})", tool_call.name, args));
                        }
                        ContentBlock::Image(_) => {}
                    }
                }
                if !thinking_parts.is_empty() {
                    parts.push(format!("[Assistant thinking]: {}", thinking_parts.join("\n")));
                }
                if !text_parts.is_empty() {
                    parts.push(format!("[Assistant]: {}", text_parts.join("\n")));
                }
                if !tool_calls.is_empty() {
                    parts.push(format!("[Assistant tool calls]: {}", tool_calls.join("; ")));
                }
            }
            Message::ToolResult(message) => {
                let content = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !content.is_empty() {
                    parts.push(format!("[Tool result]: {}", truncate_for_summary(&content, TOOL_RESULT_MAX_CHARS)));
                }
            }
        }
    }
    parts.join("\n\n")
}

/// 把 AgentMessage 序列转换成 LLM 消息后再序列化。
pub fn serialize_agent_conversation(messages: Vec<AgentMessage>) -> String {
    let harness_messages = messages.into_iter().map(HarnessMessage::Agent).collect::<Vec<_>>();
    serialize_conversation(&convert_to_llm(harness_messages))
}
