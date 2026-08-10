pub mod branch_summarization;
pub mod compaction;
pub mod utils;

use crate::{
    agent::harness::{
        session::Session,
        types::{AgentHarnessError, AgentHarnessRuntimeResult},
    },
    model::types::{Auth, Model, ThinkingLevel},
};

pub use branch_summarization::{
    collect_entries_for_branch_summary, generate_branch_summary, prepare_branch_entries, BranchPreparation,
    BranchSummaryDetails, BranchSummaryResult, CollectEntriesResult, GenerateBranchSummaryOptions,
    BRANCH_SUMMARY_PREAMBLE, BRANCH_SUMMARY_PROMPT,
};
pub use compaction::{
    calculate_context_tokens, estimate_context_tokens, estimate_tokens, extract_file_operations, find_cut_point,
    find_turn_start_index, generate_summary, generate_turn_prefix_summary, get_assistant_usage,
    get_last_assistant_usage, get_message_from_entry, get_message_from_entry_for_compaction, prepare_compaction,
    should_compact, CompactionDetails, CompactionResult, ContextUsageEstimate, CutPointResult,
    DEFAULT_COMPACTION_SETTINGS, SUMMARIZATION_PROMPT, TURN_PREFIX_SUMMARIZATION_PROMPT, UPDATE_SUMMARIZATION_PROMPT,
};
pub use utils::{
    compute_file_lists, create_file_ops, extract_file_ops_from_message, format_file_operations,
    serialize_agent_conversation, FileLists, FileOperations, SUMMARIZATION_SYSTEM_PROMPT, TOOL_RESULT_MAX_CHARS,
};

/// 压缩 Session 当前分支的历史上下文并写入 compaction 条目。
///
/// @param session 当前会话。
/// @param model 用于生成摘要的模型。
/// @param auth 模型认证信息。
/// @param thinking_level 摘要请求的思考等级。
/// @param settings 压缩运行时设置。
/// @param custom_instructions 可选的摘要附加指令。
pub async fn compact(
    session: &Session,
    model: &Model,
    auth: &Auth,
    thinking_level: Option<ThinkingLevel>,
    settings: compaction::CompactionSettings,
    custom_instructions: Option<&str>,
) -> AgentHarnessRuntimeResult<CompactionResult> {
    let branch_entries = session.get_branch(None).await;
    // 当前分支不需要再生成压缩摘要。
    let Some(preparation) = prepare_compaction(branch_entries, settings) else {
        return Ok(CompactionResult::default());
    };
    let result = compaction::compact(preparation, model, auth, custom_instructions, thinking_level).await?;
    session
        .append_compaction(
            result.summary.clone(),
            result.first_kept_entry_id.clone(),
            result.tokens_before,
            Some(serde_json::to_value(&result.details).unwrap_or(serde_json::Value::Null)),
            Some(false),
        )
        .await
        .map_err(AgentHarnessError::from)?;
    Ok(result)
}
