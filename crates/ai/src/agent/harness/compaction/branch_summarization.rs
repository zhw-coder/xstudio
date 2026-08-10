//! 会话树跨分支导航时使用的分支总结实现。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    agent::{
        harness::{
            compaction::{
                compaction::{estimate_tokens, get_message_from_entry},
                utils::{
                    compute_file_lists, extract_file_ops_from_message, format_file_operations,
                    serialize_agent_conversation, FileOperations,
                },
            },
            session::Session,
            types::SessionTreeEntry,
        },
        types::AgentMessage,
    },
    model::{
        stream::complete_simple,
        types::{
            Auth, ContentBlock, Context, Message, Model, StopReason, StreamOptions, TextContent, UserContent,
            UserMessage,
        },
    },
};

/// `generate_branch_summary` 的返回值。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryResult {
    /// 总结正文。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// 仅读未改的文件路径列表。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_files: Option<Vec<String>>,
    /// 被写入或编辑的文件路径列表。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_files: Option<Vec<String>>,
    /// 总结被取消时为 true。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aborted: Option<bool>,
    /// 总结失败时记录错误信息。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 写入 `BranchSummaryEntry.details` 中的分支文件操作快照。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummaryDetails {
    /// 累计被读取过且未被写入或编辑的文件路径列表。
    pub read_files: Vec<String>,
    /// 累计被写入或编辑过的文件路径列表。
    pub modified_files: Vec<String>,
}

/// `prepare_branch_entries` 的输出。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BranchPreparation {
    /// 按时间顺序排列的待总结消息序列。
    pub messages: Vec<AgentMessage>,
    /// 从工具调用与历史 branch_summary details 中累计得到的文件操作。
    pub file_ops: FileOperations,
    /// 入选消息的总估算 token 数。
    pub total_tokens: u64,
}

/// `collect_entries_for_branch_summary` 的输出。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectEntriesResult {
    /// 按时间顺序排列的待总结会话条目。
    pub entries: Vec<SessionTreeEntry>,
    /// 旧位置与新位置的最深公共祖先 id。
    pub common_ancestor_id: Option<String>,
}

/// `generate_branch_summary` 的可选参数。
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenerateBranchSummaryOptions<'a> {
    /// 用于总结的 Model。
    pub model: &'a Model,
    /// 调用模型所需的认证信息。
    pub auth: &'a Auth,
    /// 可选的总结额外关注点。
    pub custom_instructions: Option<String>,
    /// 为 true 时把 custom_instructions 作为完整提示词替换默认模板。
    pub replace_instructions: bool,
    /// 预留给提示词与模型响应的 token 数。
    pub reserve_tokens: Option<u64>,
}

/// 收集“从旧叶子切换到目标叶子时要总结的条目”。
pub async fn collect_entries_for_branch_summary(
    session: &Session,
    old_leaf_id: Option<&str>,
    target_id: &str,
) -> CollectEntriesResult {
    let Some(old_leaf_id) = old_leaf_id else {
        return CollectEntriesResult { entries: Vec::new(), common_ancestor_id: None };
    };

    let old_path = session
        .get_branch(Some(old_leaf_id))
        .await
        .into_iter()
        .map(|entry| entry.id().to_string())
        .collect::<HashSet<_>>();
    let target_path = session.get_branch(Some(target_id)).await;
    let common_ancestor_id =
        target_path.iter().rev().find(|entry| old_path.contains(entry.id())).map(|entry| entry.id().to_string());

    let mut entries = Vec::new();
    let mut current = Some(old_leaf_id.to_string());
    while let Some(current_id) = current {
        if Some(current_id.as_str()) == common_ancestor_id.as_deref() {
            break;
        }
        let Some(entry) = session.get_entry(&current_id).await else { break };
        current = entry.parent_id().map(str::to_string);
        entries.push(entry);
    }
    entries.reverse();

    CollectEntriesResult { entries, common_ancestor_id }
}

/// 在 token 预算约束下挑选要喂给 LLM 的分支条目。
pub fn prepare_branch_entries(entries: Vec<SessionTreeEntry>, token_budget: u64) -> BranchPreparation {
    let mut messages = Vec::<AgentMessage>::new();
    let mut file_ops = FileOperations::default();
    let mut total_tokens = 0_u64;

    for entry in &entries {
        if let SessionTreeEntry::BranchSummary { details: Some(details), from_hook, .. } = entry {
            if from_hook.unwrap_or(false) {
                continue;
            }
            merge_branch_summary_details(details, &mut file_ops);
        }
    }

    for entry in entries.into_iter().rev() {
        let is_summary_entry =
            matches!(entry, SessionTreeEntry::Compaction { .. } | SessionTreeEntry::BranchSummary { .. });
        let Some(message) = get_branch_message_from_entry(entry) else { continue };
        extract_file_ops_from_message(&message, &mut file_ops);
        let tokens = estimate_tokens(std::slice::from_ref(&message));
        if token_budget > 0 && total_tokens.saturating_add(tokens) > token_budget {
            if is_summary_entry && (total_tokens as f64) < (token_budget as f64) * 0.9 {
                messages.insert(0, message);
                total_tokens = total_tokens.saturating_add(tokens);
            }
            break;
        }
        messages.insert(0, message);
        total_tokens = total_tokens.saturating_add(tokens);
    }

    BranchPreparation { messages, file_ops, total_tokens }
}

/// 把历史分支总结 details 合并到文件操作累加器。
fn merge_branch_summary_details(details: &Value, file_ops: &mut FileOperations) {
    for key in ["readFiles", "read_files"] {
        if let Some(files) = details.get(key).and_then(Value::as_array) {
            for file in files.iter().filter_map(Value::as_str) {
                file_ops.read.insert(file.to_string());
            }
        }
    }
    for key in ["modifiedFiles", "modified_files"] {
        if let Some(files) = details.get(key).and_then(Value::as_array) {
            for file in files.iter().filter_map(Value::as_str) {
                file_ops.edited.insert(file.to_string());
            }
        }
    }
}

/// 把一条会话条目转换成分支总结用 AgentMessage。
fn get_branch_message_from_entry(entry: SessionTreeEntry) -> Option<AgentMessage> {
    match entry {
        SessionTreeEntry::Message { message: AgentMessage::ToolResult(_), .. } => None,
        _ => get_message_from_entry(entry),
    }
}

/// 拼接到分支总结正文最前面的固定提示词。
pub const BRANCH_SUMMARY_PREAMBLE: &str =
    "The user explored a different conversation branch before returning here.\nSummary of that exploration:\n\n";

/// 让 LLM 输出“分支结构化摘要”的提示词模板。
pub const BRANCH_SUMMARY_PROMPT: &str = "Create a structured summary of this conversation branch for context when returning later.\n\nUse this EXACT format:\n\n## Goal\n[What was the user trying to accomplish in this branch?]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Work that was started but not finished]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [What should happen next to continue this work]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

/// 调用 LLM 生成分支总结。
pub async fn generate_branch_summary(
    entries: Vec<SessionTreeEntry>,
    options: GenerateBranchSummaryOptions<'_>,
) -> Result<BranchSummaryResult, crate::model::types::StreamError> {
    let reserve_tokens = options.reserve_tokens.unwrap_or(16_384);
    let context_window = if options.model.context_window > 0 { options.model.context_window } else { 128_000 };
    let token_budget = context_window.saturating_sub(reserve_tokens);
    let preparation = prepare_branch_entries(entries, token_budget);

    if preparation.messages.is_empty() {
        return Ok(BranchSummaryResult { summary: Some("No content to summarize".to_string()), ..Default::default() });
    }

    let conversation_text = serialize_agent_conversation(preparation.messages);
    let instructions = if options.replace_instructions {
        options.custom_instructions.as_deref().unwrap_or(BRANCH_SUMMARY_PROMPT).to_string()
    } else if let Some(custom_instructions) = &options.custom_instructions {
        format!("{BRANCH_SUMMARY_PROMPT}\n\nAdditional focus: {custom_instructions}")
    } else {
        BRANCH_SUMMARY_PROMPT.to_string()
    };
    let prompt_text = format!("<conversation>\n{conversation_text}\n</conversation>\n\n{instructions}");
    let summarization_messages = vec![Message::User(UserMessage {
        content: UserContent::Blocks(vec![ContentBlock::Text(TextContent { text: prompt_text, text_signature: None })]),
        timestamp: time::OffsetDateTime::now_utc().unix_timestamp(),
    })];

    let stream_options = StreamOptions { max_tokens: Some(2048), ..Default::default() };
    let response = complete_simple(
        options.model,
        Context { system_prompt: None, messages: summarization_messages, tools: Vec::new() },
        &stream_options,
        options.auth,
    )
    .await?;

    if matches!(response.stop_reason, StopReason::Aborted) {
        return Ok(BranchSummaryResult { aborted: Some(true), ..Default::default() });
    }
    if matches!(response.stop_reason, StopReason::Error) {
        return Ok(BranchSummaryResult {
            error: Some(response.error_message.unwrap_or_else(|| "Summarization failed".to_string())),
            ..Default::default()
        });
    }

    let mut summary = response
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    summary = format!("{BRANCH_SUMMARY_PREAMBLE}{summary}");
    let file_lists = compute_file_lists(&preparation.file_ops);
    summary.push_str(&format_file_operations(&file_lists.read_files, &file_lists.modified_files));

    Ok(BranchSummaryResult {
        summary: Some(if summary.is_empty() { "No summary generated".to_string() } else { summary }),
        read_files: Some(file_lists.read_files),
        modified_files: Some(file_lists.modified_files),
        ..Default::default()
    })
}
