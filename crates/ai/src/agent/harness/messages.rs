//! Harness 层在底层 LLM `Message` 之外定义的扩展 AgentMessage 类型集合，并提供把这些扩展消息
//! 转换回 LLM 可消费的 `Message[]` 的 `convert_to_llm` 实现。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    agent::{harness::types::CustomMessageContent, types::AgentMessage},
    model::types::{ContentBlock, ImageContent, Message, TextContent, UserContent, UserMessage},
};

/// 压缩总结嵌入到对话中时使用的文本前缀。
pub const COMPACTION_SUMMARY_PREFIX: &str =
    "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";
/// 压缩总结嵌入到对话中时使用的文本后缀。
pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";
/// 分支总结嵌入到对话中时使用的文本前缀。
pub const BRANCH_SUMMARY_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";
/// 分支总结嵌入到对话中时使用的文本后缀。
pub const BRANCH_SUMMARY_SUFFIX: &str = "</summary>";

/// 表示一次 shell / bash 命令执行结果的扩展 AgentMessage。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BashExecutionMessage {
    /// 实际执行的命令字符串。
    pub command: String,
    /// 合并输出。
    pub output: String,
    /// 命令退出码。
    pub exit_code: Option<i32>,
    /// 命令是否被取消。
    pub cancelled: bool,
    /// 输出是否被截断。
    pub truncated: bool,
    /// 完整输出路径。
    pub full_output_path: Option<String>,
    /// 消息时间戳。
    pub timestamp: i64,
    /// 是否排除出 LLM 上下文。
    pub exclude_from_context: Option<bool>,
}

/// 应用层自定义的扩展 AgentMessage。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomMessage {
    /// 业务子类型标识。
    pub custom_type: String,
    /// 消息内容。
    pub content: CustomMessageContent,
    /// 是否在 UI 中显示。
    pub display: bool,
    /// details 负载。
    pub details: Option<Value>,
    /// 消息时间戳。
    pub timestamp: i64,
}

/// 分支总结消息。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryMessage {
    /// 分支总结正文。
    pub summary: String,
    /// 来源分支条目 id。
    pub from_id: String,
    /// 消息时间戳。
    pub timestamp: i64,
}

/// 压缩总结消息。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryMessage {
    /// 压缩总结正文。
    pub summary: String,
    /// 压缩前 token 估算。
    pub tokens_before: u64,
    /// 消息时间戳。
    pub timestamp: i64,
}

/// Harness 扩展消息联合类型。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role")]
pub enum HarnessMessage {
    /// Bash 执行消息。
    #[serde(rename = "bashExecution")]
    BashExecution(BashExecutionMessage),
    /// 自定义消息。
    #[serde(rename = "custom")]
    Custom(CustomMessage),
    /// 分支总结。
    #[serde(rename = "branchSummary")]
    BranchSummary(BranchSummaryMessage),
    /// 压缩总结。
    #[serde(rename = "compactionSummary")]
    CompactionSummary(CompactionSummaryMessage),
    /// 基础 Agent 消息。
    #[serde(rename = "agent")]
    Agent(AgentMessage),
}

/// 将 BashExecutionMessage 渲染为提交给 LLM 的纯文本表示。
pub fn bash_execution_to_text(msg: &BashExecutionMessage) -> String {
    let mut text = format!("Ran `{}`\n", msg.command);
    if msg.output.is_empty() {
        text.push_str("(no output)");
    } else {
        text.push_str("```\n");
        text.push_str(&msg.output);
        text.push_str("\n```");
    }
    if msg.cancelled {
        text.push_str("\n\n(command cancelled)");
    } else if msg.exit_code.is_some_and(|code| code != 0) {
        text.push_str(&format!("\n\nCommand exited with code {}", msg.exit_code.unwrap_or_default()));
    }
    if msg.truncated {
        if let Some(path) = &msg.full_output_path {
            text.push_str(&format!("\n\n[Output truncated. Full output: {path}]"));
        }
    }
    text
}

/// 构造一条分支总结消息。
pub fn create_branch_summary_message(summary: String, from_id: String, timestamp_ms: i64) -> BranchSummaryMessage {
    BranchSummaryMessage { summary, from_id, timestamp: timestamp_ms }
}

/// 构造一条压缩总结消息。
pub fn create_compaction_summary_message(
    summary: String,
    tokens_before: u64,
    timestamp_ms: i64,
) -> CompactionSummaryMessage {
    CompactionSummaryMessage { summary, tokens_before, timestamp: timestamp_ms }
}

/// 构造一条自定义消息。
pub fn create_custom_message(
    custom_type: String,
    content: CustomMessageContent,
    display: bool,
    details: Option<Value>,
    timestamp_ms: i64,
) -> CustomMessage {
    CustomMessage { custom_type, content, display, details, timestamp: timestamp_ms }
}

/// Harness 层默认的 convertToLlm 实现。
pub fn convert_to_llm(messages: Vec<HarnessMessage>) -> Vec<Message> {
    messages.into_iter().filter_map(harness_message_to_llm).collect()
}

/// 将单条 HarnessMessage 转为 LLM Message。
fn harness_message_to_llm(message: HarnessMessage) -> Option<Message> {
    match message {
        HarnessMessage::BashExecution(message) => {
            if message.exclude_from_context.unwrap_or(false) {
                None
            } else {
                Some(user_text_message(bash_execution_to_text(&message), message.timestamp))
            }
        }
        HarnessMessage::Custom(message) => Some(Message::User(UserMessage {
            content: match message.content {
                CustomMessageContent::Text(text) => {
                    UserContent::Blocks(vec![ContentBlock::Text(TextContent { text, text_signature: None })])
                }
                CustomMessageContent::Blocks(blocks) => UserContent::Blocks(blocks),
            },
            timestamp: message.timestamp,
        })),
        HarnessMessage::BranchSummary(message) => Some(user_text_message(
            format!("{BRANCH_SUMMARY_PREFIX}{}{BRANCH_SUMMARY_SUFFIX}", message.summary),
            message.timestamp,
        )),
        HarnessMessage::CompactionSummary(message) => Some(user_text_message(
            format!("{COMPACTION_SUMMARY_PREFIX}{}{COMPACTION_SUMMARY_SUFFIX}", message.summary),
            message.timestamp,
        )),
        HarnessMessage::Agent(message) => message.into_llm_message(),
    }
}

/// 构造纯文本 user message。
fn user_text_message(text: String, timestamp: i64) -> Message {
    Message::User(UserMessage {
        content: UserContent::Blocks(vec![ContentBlock::Text(TextContent { text, text_signature: None })]),
        timestamp,
    })
}

/// 根据文本与图片构造 user message 内容块。
pub fn user_content_with_images(text: String, images: Vec<ImageContent>) -> UserContent {
    let mut blocks = vec![ContentBlock::Text(TextContent { text, text_signature: None })];
    blocks.extend(images.into_iter().map(ContentBlock::Image));
    UserContent::Blocks(blocks)
}
