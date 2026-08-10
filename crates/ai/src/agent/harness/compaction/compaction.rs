//! 这里主要提供可复用的上下文压缩纯逻辑：token 估算、cut point 计算、文件操作抽取和
//! LLM 总结调用入口。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    agent::{
        harness::{
            compaction::utils::{
                compute_file_lists, extract_file_ops_from_message, format_file_operations,
                serialize_agent_conversation, FileLists, FileOperations, SUMMARIZATION_SYSTEM_PROMPT,
            },
            messages::{convert_to_llm, HarnessMessage},
            session::build_session_context,
            types::SessionTreeEntry,
        },
        types::AgentMessage,
    },
    model::{
        stream::complete_simple,
        types::{
            AssistantMessage, Auth, ContentBlock, Context, Message, Model, StopReason, StreamOptions, TextContent,
            ThinkingLevel, Usage, UserContent, UserMessage,
        },
    },
};

/// 压缩摘要中附带的文件操作详情。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionDetails {
    /// 历史中仅读取未修改的文件列表。
    pub read_files: Vec<String>,
    /// 历史中被写入或编辑的文件列表。
    pub modified_files: Vec<String>,
}

/// 压缩调用的运行时设置。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSettings {
    /// 是否启用自动压缩。
    pub enabled: bool,
    /// 预留给总结请求和后续上下文的 token 数。
    pub reserve_tokens: u64,
    /// 压缩时尽量保留最近多少 token。
    pub keep_recent_tokens: u64,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        DEFAULT_COMPACTION_SETTINGS
    }
}

/// 默认压缩配置，与 TypeScript 实现保持一致。
pub const DEFAULT_COMPACTION_SETTINGS: CompactionSettings =
    CompactionSettings { enabled: true, reserve_tokens: 16_384, keep_recent_tokens: 20_000 };

/// 可写入会话树的压缩结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionResult {
    /// 总结正文。
    pub summary: String,
    /// 第一条保留条目的 UUID。
    pub first_kept_entry_id: String,
    /// 压缩前 token 估算值。
    pub tokens_before: u64,
    /// 文件操作详情。
    pub details: CompactionDetails,
}

/// 估算出的上下文使用量。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsageEstimate {
    /// 上下文 token 估算。
    pub tokens: u64,
    /// 估算来源。
    pub source: String,
}

/// cut point 查找结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CutPointResult {
    /// 第一条保留条目的索引。
    pub first_kept_entry_index: usize,
    /// 如果切分落在轮次中段，此字段表示该轮起点索引。
    pub turn_start_index: usize,
    /// cut point 是否落在轮次中段。
    pub is_split_turn: bool,
}

/// `prepare_compaction` 的输出——一份“压缩前的预备数据”。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPreparation {
    /// 第一条要保留条目的 UUID。
    pub first_kept_entry_id: String,
    /// 将被总结并丢弃的消息序列。
    pub messages_to_summarize: Vec<AgentMessage>,
    /// 当切分落在轮次中段时，对应“被切走的轮次前缀”消息序列。
    pub turn_prefix_messages: Vec<AgentMessage>,
    /// 切分点是否落在轮次中段。
    pub is_split_turn: bool,
    /// 压缩前估算的对话上下文 token 数。
    pub tokens_before: u64,
    /// 上一次压缩的总结正文。
    pub previous_summary: Option<String>,
    /// 从待总结消息与历史压缩中累计的文件操作。
    pub file_ops: FileOperations,
    /// 压缩配置。
    pub settings: CompactionSettings,
}

/// 计算 `Usage` 中会进入下一轮上下文的 token 数。
pub fn calculate_context_tokens(usage: &Usage) -> u64 {
    usage.input + usage.output + usage.cache_read + usage.cache_write
}

/// 从 assistant 消息中读取 usage。
pub fn get_assistant_usage(message: &AgentMessage) -> Option<&Usage> {
    match message {
        AgentMessage::Assistant(message) => Some(&message.usage),
        _ => None,
    }
}

/// 返回消息序列中最后一条 assistant usage。
pub fn get_last_assistant_usage(messages: &[AgentMessage]) -> Option<&Usage> {
    messages.iter().rev().find_map(get_assistant_usage)
}

/// 估算当前上下文 token 数；优先使用最后一次 provider usage。
pub fn estimate_context_tokens(messages: &[AgentMessage]) -> ContextUsageEstimate {
    if let Some(usage) = get_last_assistant_usage(messages) {
        return ContextUsageEstimate { tokens: calculate_context_tokens(usage), source: "usage".to_string() };
    }
    ContextUsageEstimate { tokens: estimate_tokens(messages), source: "estimate".to_string() }
}

/// 判断是否应该触发压缩。
pub fn should_compact(context_tokens: u64, context_window: u64, settings: &CompactionSettings) -> bool {
    settings.enabled && context_tokens.saturating_add(settings.reserve_tokens) >= context_window
}

/// 粗略估算 AgentMessage 序列 token 数。
pub fn estimate_tokens(messages: &[AgentMessage]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

/// 粗略估算单条消息 token 数。
fn estimate_message_tokens(message: &AgentMessage) -> u64 {
    match message {
        AgentMessage::User(message) => estimate_user_content(&message.content),
        AgentMessage::Assistant(message) => estimate_content_blocks(&message.content),
        AgentMessage::ToolResult(message) => estimate_content_blocks(&message.content),
        AgentMessage::Custom { payload, .. } => estimate_string_tokens(&payload.to_string()),
    }
}

/// 粗略估算用户内容 token 数。
fn estimate_user_content(content: &UserContent) -> u64 {
    match content {
        UserContent::Text(text) => estimate_string_tokens(text),
        UserContent::Blocks(blocks) => estimate_content_blocks(blocks),
    }
}

/// 粗略估算内容块 token 数。
fn estimate_content_blocks(content: &[ContentBlock]) -> u64 {
    content
        .iter()
        .map(|block| match block {
            ContentBlock::Text(text) => estimate_string_tokens(&text.text),
            ContentBlock::Thinking(thinking) => estimate_string_tokens(&thinking.thinking),
            ContentBlock::Image(_) => 1_000,
            ContentBlock::ToolCall(tool_call) => {
                estimate_string_tokens(&tool_call.name)
                    + estimate_string_tokens(&serde_json::to_string(&tool_call.arguments).unwrap_or_default())
            }
        })
        .sum()
}

/// 经验 token 估算：约 4 字符一个 token。
fn estimate_string_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).saturating_add(3) / 4
}

/// 找到给定索引所在轮次的 user 起点。
pub fn find_turn_start_index(entries: &[SessionTreeEntry], index: usize, boundary_start: usize) -> usize {
    let mut current = index.min(entries.len());
    while current > boundary_start {
        if matches!(
            entries.get(current - 1),
            Some(SessionTreeEntry::Message { message: AgentMessage::User(_), .. })
                | Some(SessionTreeEntry::BranchSummary { .. })
                | Some(SessionTreeEntry::CustomMessage { .. })
        ) {
            return current - 1;
        }
        current -= 1;
    }
    boundary_start
}

/// 返回可作为压缩保留起点的条目下标。
///
/// `ToolResult` 不能作为切点，否则会保留没有前置 `ToolCall` 的工具结果。
fn find_valid_cut_points(entries: &[SessionTreeEntry], boundary_start: usize, boundary_end: usize) -> Vec<usize> {
    (boundary_start..boundary_end)
        .filter(|&index| {
            matches!(
                entries.get(index),
                Some(SessionTreeEntry::Message { message: AgentMessage::User(_) | AgentMessage::Assistant(_), .. })
                    | Some(SessionTreeEntry::BranchSummary { .. })
                    | Some(SessionTreeEntry::CustomMessage { .. })
            )
        })
        .collect()
}

/// 在会话路径中选择一条合法压缩 cut point。
pub fn find_cut_point(
    entries: &[SessionTreeEntry],
    boundary_start: usize,
    boundary_end: usize,
    keep_recent_tokens: u64,
) -> CutPointResult {
    if boundary_start >= boundary_end {
        return CutPointResult {
            first_kept_entry_index: boundary_end.saturating_sub(1),
            turn_start_index: boundary_end.saturating_sub(1),
            is_split_turn: false,
        };
    }

    let cut_points = find_valid_cut_points(entries, boundary_start, boundary_end);
    if cut_points.is_empty() {
        return CutPointResult {
            first_kept_entry_index: boundary_start,
            turn_start_index: boundary_start,
            is_split_turn: false,
        };
    }

    let mut tokens = 0_u64;
    let mut first_kept = cut_points[0];
    for index in (boundary_start..boundary_end).rev() {
        let SessionTreeEntry::Message { message, .. } = &entries[index] else { continue };
        tokens = tokens.saturating_add(estimate_message_tokens(message));
        if tokens >= keep_recent_tokens {
            if let Some(&cut_point) = cut_points.iter().find(|&&cut_point| cut_point >= index) {
                first_kept = cut_point;
            }
            break;
        }
    }

    let is_user_message =
        matches!(entries.get(first_kept), Some(SessionTreeEntry::Message { message: AgentMessage::User(_), .. }));
    let turn_start =
        if is_user_message { first_kept } else { find_turn_start_index(entries, first_kept, boundary_start) };
    let is_split_turn = !is_user_message && turn_start < first_kept;
    CutPointResult { first_kept_entry_index: first_kept, turn_start_index: turn_start, is_split_turn }
}

/// 从条目中提取普通上下文消息。
pub fn get_message_from_entry(entry: SessionTreeEntry) -> Option<AgentMessage> {
    match entry {
        SessionTreeEntry::Message { message, .. } => Some(message),
        SessionTreeEntry::Compaction { summary, tokens_before, base, .. } => Some(AgentMessage::Custom {
            kind: "compactionSummary".to_string(),
            payload: serde_json::json!({ "summary": summary, "tokensBefore": tokens_before, "timestamp": base.timestamp }),
        }),
        SessionTreeEntry::BranchSummary { from_id, summary, base, .. } => Some(AgentMessage::Custom {
            kind: "branchSummary".to_string(),
            payload: serde_json::json!({ "fromId": from_id, "summary": summary, "timestamp": base.timestamp }),
        }),
        SessionTreeEntry::CustomMessage { custom_type, content, .. } => Some(AgentMessage::Custom {
            kind: custom_type,
            payload: serde_json::to_value(content).unwrap_or(Value::Null),
        }),
        _ => None,
    }
}

/// 从条目中提取参与压缩的消息。
pub fn get_message_from_entry_for_compaction(entry: SessionTreeEntry) -> Option<AgentMessage> {
    match entry {
        SessionTreeEntry::Compaction { .. } => None,
        _ => get_message_from_entry(entry),
    }
}

/// 从待压缩消息和上一次压缩详情中累计文件操作。
pub fn extract_file_operations(
    messages: &[AgentMessage],
    entries: &[SessionTreeEntry],
    prev_compaction_index: Option<usize>,
) -> FileOperations {
    let mut file_ops = FileOperations::default();
    if let Some(index) = prev_compaction_index {
        if let Some(SessionTreeEntry::Compaction { details: Some(details), .. }) = entries.get(index) {
            merge_previous_details(details, &mut file_ops);
        }
    }
    for message in messages {
        extract_file_ops_from_message(message, &mut file_ops);
    }
    file_ops
}

/// 合并历史压缩中保存的文件操作详情。
fn merge_previous_details(details: &Value, file_ops: &mut FileOperations) {
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

/// 在不调用 LLM 的前提下计算压缩需要的全部预备数据。
pub fn prepare_compaction(
    path_entries: Vec<SessionTreeEntry>,
    settings: CompactionSettings,
) -> Option<CompactionPreparation> {
    if matches!(path_entries.last(), Some(SessionTreeEntry::Compaction { .. })) {
        return None;
    }

    let prev_compaction_index =
        path_entries.iter().rposition(|entry| matches!(entry, SessionTreeEntry::Compaction { .. }));
    let mut previous_summary = None;
    let mut boundary_start = 0_usize;
    if let Some(index) = prev_compaction_index {
        if let SessionTreeEntry::Compaction { first_kept_entry_id, .. } = &path_entries[index] {
            boundary_start =
                path_entries.iter().position(|entry| entry.id() == first_kept_entry_id).unwrap_or(index + 1);
        }
    }

    let boundary_end = path_entries.len();
    if boundary_start >= boundary_end {
        return None;
    }

    let tokens_before = estimate_context_tokens(&build_session_context(&path_entries).messages).tokens;
    let cut_point = find_cut_point(&path_entries, boundary_start, boundary_end, settings.keep_recent_tokens);
    let first_kept_entry = path_entries.get(cut_point.first_kept_entry_index)?;
    let first_kept_entry_id = first_kept_entry.id().to_string();
    let history_end =
        if cut_point.is_split_turn { cut_point.turn_start_index } else { cut_point.first_kept_entry_index };

    let mut file_ops = FileOperations::default();
    if let Some(index) = prev_compaction_index {
        if let Some(SessionTreeEntry::Compaction { details: Some(details), .. }) = path_entries.get(index) {
            merge_previous_details(details, &mut file_ops);
        }
    }

    let mut messages_to_summarize = Vec::new();
    let mut turn_prefix_messages = Vec::new();
    for (index, entry) in path_entries.into_iter().enumerate() {
        if Some(index) == prev_compaction_index {
            if let SessionTreeEntry::Compaction { summary, .. } = entry {
                previous_summary = Some(summary);
            }
            continue;
        }

        let in_history = (boundary_start..history_end).contains(&index);
        let in_turn_prefix =
            cut_point.is_split_turn && (cut_point.turn_start_index..cut_point.first_kept_entry_index).contains(&index);
        if !in_history && !in_turn_prefix {
            continue;
        }
        if let Some(message) = get_message_from_entry_for_compaction(entry) {
            extract_file_ops_from_message(&message, &mut file_ops);
            if in_history {
                messages_to_summarize.push(message);
            } else {
                turn_prefix_messages.push(message);
            }
        }
    }

    Some(CompactionPreparation {
        first_kept_entry_id,
        messages_to_summarize,
        turn_prefix_messages,
        is_split_turn: cut_point.is_split_turn,
        tokens_before,
        previous_summary,
        file_ops,
        settings,
    })
}

/// 首次压缩总结提示词模板。
pub const SUMMARIZATION_PROMPT: &str = "Create a concise but comprehensive summary of the conversation above. Focus on the user's goals, important decisions, files read or modified, bugs fixed, and any remaining tasks. Preserve details that will help continue the work after context compaction.";

/// 迭代式压缩总结更新提示词模板。
pub const UPDATE_SUMMARIZATION_PROMPT: &str = "Update the previous summary with the new conversation. Keep the result concise, remove obsolete details, and preserve actionable context for continuing the work.";

/// 切分点落在轮次中段时，专门用于总结“被切走前缀”的提示词模板。
pub const TURN_PREFIX_SUMMARIZATION_PROMPT: &str = "This is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.\n\nSummarize the prefix to provide context for the retained suffix:\n\n## Original Request\n[What did the user ask for in this turn?]\n\n## Early Progress\n- [Key decisions and work done in the prefix]\n\n## Context for Suffix\n- [Information needed to understand the retained recent work]\n\nBe concise. Focus on what's needed to understand the kept suffix.";

/// 基于消息调用 LLM 生成历史总结。
pub async fn generate_summary(
    current_messages: Vec<AgentMessage>,
    model: &Model,
    reserve_tokens: u64,
    auth: &Auth,
    custom_instructions: Option<&str>,
    previous_summary: Option<String>,
    thinking_level: Option<ThinkingLevel>,
) -> Result<String, crate::model::types::StreamError> {
    let max_tokens = ((reserve_tokens as f64) * 0.8).floor() as u64;
    let mut base_prompt = if previous_summary.is_some() {
        UPDATE_SUMMARIZATION_PROMPT.to_string()
    } else {
        SUMMARIZATION_PROMPT.to_string()
    };
    if let Some(custom_instructions) = custom_instructions {
        base_prompt.push_str("\n\nAdditional focus: ");
        base_prompt.push_str(custom_instructions);
    }

    let conversation_text = serialize_agent_conversation(current_messages);
    let mut prompt_text = format!("<conversation>\n{conversation_text}\n</conversation>\n\n");
    if let Some(previous_summary) = previous_summary {
        prompt_text.push_str(&format!("<previous-summary>\n{previous_summary}\n</previous-summary>\n\n"));
    }
    prompt_text.push_str(&base_prompt);

    summarize_prompt(model, max_tokens, auth, thinking_level, prompt_text).await
}

/// 基于 `CompactionPreparation` 实际调用 LLM 完成压缩。
pub async fn compact(
    preparation: CompactionPreparation,
    model: &Model,
    auth: &Auth,
    custom_instructions: Option<&str>,
    thinking_level: Option<ThinkingLevel>,
) -> Result<CompactionResult, crate::model::types::StreamError> {
    let CompactionPreparation {
        first_kept_entry_id,
        messages_to_summarize,
        turn_prefix_messages,
        is_split_turn,
        tokens_before,
        previous_summary,
        file_ops,
        settings,
    } = preparation;

    let summary = if is_split_turn && !turn_prefix_messages.is_empty() {
        let history = if messages_to_summarize.is_empty() {
            "No prior history.".to_string()
        } else {
            generate_summary(
                messages_to_summarize,
                model,
                settings.reserve_tokens,
                auth,
                custom_instructions,
                previous_summary,
                thinking_level,
            )
            .await?
        };
        let turn_prefix =
            generate_turn_prefix_summary(turn_prefix_messages, model, settings.reserve_tokens, auth, thinking_level)
                .await?;
        format!("{history}\n\n---\n\n**Turn Context (split turn):**\n\n{turn_prefix}")
    } else {
        generate_summary(
            messages_to_summarize,
            model,
            settings.reserve_tokens,
            auth,
            custom_instructions,
            previous_summary,
            thinking_level,
        )
        .await?
    };

    let FileLists { read_files, modified_files } = compute_file_lists(&file_ops);
    let summary = format!("{}{}", summary, format_file_operations(&read_files, &modified_files));
    Ok(CompactionResult {
        summary,
        first_kept_entry_id,
        tokens_before,
        details: CompactionDetails { read_files, modified_files },
    })
}

/// 为“被切走的轮次前缀”生成一段紧凑摘要。
pub async fn generate_turn_prefix_summary(
    messages: Vec<AgentMessage>,
    model: &Model,
    reserve_tokens: u64,
    auth: &Auth,
    thinking_level: Option<ThinkingLevel>,
) -> Result<String, crate::model::types::StreamError> {
    let max_tokens = ((reserve_tokens as f64) * 0.5).floor() as u64;
    let conversation_text = serialize_agent_conversation(messages);
    let prompt_text =
        format!("<conversation>\n{conversation_text}\n</conversation>\n\n{TURN_PREFIX_SUMMARIZATION_PROMPT}");
    summarize_prompt(model, max_tokens, auth, thinking_level, prompt_text).await
}

/// 调用简化完成接口并提取纯文本内容。
async fn summarize_prompt(
    model: &Model,
    max_tokens: u64,
    auth: &Auth,
    thinking_level: Option<ThinkingLevel>,
    prompt_text: String,
) -> Result<String, crate::model::types::StreamError> {
    let summarization_messages = vec![Message::User(UserMessage {
        content: UserContent::Blocks(vec![ContentBlock::Text(TextContent { text: prompt_text, text_signature: None })]),
        timestamp: time::OffsetDateTime::now_utc().unix_timestamp(),
    })];
    let options = StreamOptions {
        max_tokens: Some(max_tokens.min(u32::MAX as u64) as u32),
        reasoning: thinking_level,
        ..Default::default()
    };
    let response = complete_simple(
        model,
        Context {
            system_prompt: Some(SUMMARIZATION_SYSTEM_PROMPT.to_string()),
            messages: summarization_messages,
            tools: Vec::new(),
        },
        &options,
        auth,
    )
    .await?;
    Ok(extract_assistant_text(response))
}

/// 从 AssistantMessage 中提取纯文本内容。
fn extract_assistant_text(response: AssistantMessage) -> String {
    if matches!(response.stop_reason, StopReason::Error) {
        return response.error_message.unwrap_or_else(|| "Unknown error".to_string());
    }
    response
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 保留对 TS `convertToLlm + serializeConversation` 组合的显式入口。
pub fn serialize_conversation(messages: Vec<AgentMessage>) -> String {
    let harness_messages = messages.into_iter().map(HarnessMessage::Agent).collect::<Vec<_>>();
    let llm_messages = convert_to_llm(harness_messages);
    crate::agent::harness::compaction::utils::serialize_conversation(&llm_messages)
}
