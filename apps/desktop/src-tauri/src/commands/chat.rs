use ai::{
    agent::harness::{
        compaction::compaction::CompactionResult, NavigateTreeResult, SessionContext,
        SessionMetadata,
    },
    model::AssistantMessage,
};

use crate::{
    dto::{
        ChatAbortInput, ChatCompactInput, ChatCreateSessionInput, ChatDeleteSessionInput,
        ChatEditAndPromptUserMessageInput, ChatForkSessionInput, ChatListResourceNamesInput,
        ChatListSessionsInput, ChatOpenSessionInput, ChatPromptInput, ChatResolveToolApprovalInput,
        ChatResourceNameOutput, ChatSetModelInput, ChatSetSessionNameInput,
        ChatSetStreamOptionsInput, ChatSetThinkingLevelInput, ChatSetToolsInput, ChatSkillInput,
        ChatTemplateInput, ChatWithdrawTurnInput,
    },
    error::command_error,
    services,
};

/// 返回全部可用会话仓储名称。
#[tauri::command]
pub fn list_session_repos() -> Vec<String> {
    services::chat::list_repos()
}

/// 查询会话列表。
/// @param app Tauri 应用句柄。
/// @param input 会话列表请求。
#[tauri::command]
pub async fn list_chat_sessions(
    app: tauri::AppHandle,
    input: ChatListSessionsInput,
) -> Result<Vec<SessionMetadata>, String> {
    services::chat::list_sessions(&app, input)
        .await
        .map_err(command_error)
}

/// 查询具体会话上下文，并在全局 Context 中缓存 AgentHarness。
/// @param app Tauri 应用句柄。
/// @param input 打开会话请求。
#[tauri::command]
pub async fn open_chat_session(
    app: tauri::AppHandle,
    input: ChatOpenSessionInput,
) -> Result<SessionContext, String> {
    services::chat::open_session(&app, input)
        .await
        .map_err(command_error)
}

/// 创建会话，并在全局 Context 中缓存 AgentHarness。
/// @param app Tauri 应用句柄。
/// @param input 创建会话请求。
#[tauri::command]
pub async fn create_chat_session(
    app: tauri::AppHandle,
    input: ChatCreateSessionInput,
) -> Result<SessionMetadata, String> {
    services::chat::create_session(&app, input)
        .await
        .map_err(command_error)
}

/// 基于会话树节点创建独立新会话。
/// @param app Tauri 应用句柄。
/// @param input Fork 会话请求。
#[tauri::command]
pub async fn fork_chat_session(
    app: tauri::AppHandle,
    input: ChatForkSessionInput,
) -> Result<SessionMetadata, String> {
    services::chat::fork_session(&app, input)
        .await
        .map_err(command_error)
}

/// 删除会话，并从全局 Context 中移除对应 AgentHarness。
/// @param input 删除会话请求。
#[tauri::command]
pub async fn delete_chat_session(input: ChatDeleteSessionInput) -> Result<(), String> {
    services::chat::delete_session(input)
        .await
        .map_err(command_error)
}

/// 对已缓存会话发起 prompt。
/// @param app Tauri 应用句柄。
/// @param input prompt 请求。
#[tauri::command]
pub async fn prompt_chat(
    app: tauri::AppHandle,
    input: ChatPromptInput,
) -> Result<Option<AssistantMessage>, String> {
    services::chat::prompt(&app, input)
        .await
        .map_err(command_error)
}

/// 终止已缓存会话的当前 run。
/// @param input 终止请求。
#[tauri::command]
pub async fn abort_chat(input: ChatAbortInput) -> Result<(), String> {
    services::chat::abort(input).await.map_err(command_error)
}

/// 结算客户端工具审批请求。
/// @param input 审批结算请求。
#[tauri::command]
pub async fn resolve_chat_tool_approval(input: ChatResolveToolApprovalInput) -> Result<(), String> {
    services::chat::resolve_tool_approval(input)
        .await
        .map_err(command_error)
}

/// 查询已缓存会话 Harness 中可调用的模板和 Skill 资源名称。
/// @param app Tauri 应用句柄。
/// @param input 资源名称查询请求。
/// @returns 第一个元素为模板，第二个元素为 Skill。
#[tauri::command]
pub async fn list_chat_resources_names(
    app: tauri::AppHandle,
    input: ChatListResourceNamesInput,
) -> Result<Vec<Vec<ChatResourceNameOutput>>, String> {
    services::chat::list_resources_names(&app, input)
        .await
        .map_err(command_error)
}

/// 压缩已缓存会话的历史上下文。
/// @param input 会话压缩请求。
#[tauri::command]
pub async fn compact_chat(
    app: tauri::AppHandle,
    input: ChatCompactInput,
) -> Result<CompactionResult, String> {
    services::chat::compact_session(&app, input)
        .await
        .map_err(command_error)
}

/// 回撤一条用户消息及其后续活跃分支内容。
///
/// 运行中的会话会先被中止并落盘；回撤不会删除原分支，调用方保留原分支条目 id 后，可在后续树导航中恢复。
/// @param input 回撤请求。
#[tauri::command]
pub async fn withdraw_chat_turn(
    input: ChatWithdrawTurnInput,
) -> Result<NavigateTreeResult, String> {
    services::chat::withdraw_turn(input)
        .await
        .map_err(command_error)
}

/// 在当前会话中编辑用户消息并重新发送。
///
/// 若会话正在运行，先中止当前 run，再回撤到编辑消息之前的分支。
/// @param input 用户消息编辑与发送请求。
#[tauri::command]
pub async fn edit_and_prompt_chat_user_message(
    input: ChatEditAndPromptUserMessageInput,
) -> Result<(), String> {
    services::chat::edit_and_prompt_user_message(input)
        .await
        .map_err(command_error)
}

/// 对已缓存会话发起 skill。
/// @param app Tauri 应用句柄。
/// @param input skill 请求。
#[tauri::command]
pub async fn skill_chat(
    app: tauri::AppHandle,
    input: ChatSkillInput,
) -> Result<AssistantMessage, String> {
    services::chat::skill(&app, input)
        .await
        .map_err(command_error)
}

/// 对已缓存会话发起 prompt template。
/// @param app Tauri 应用句柄。
/// @param input prompt template 请求。
#[tauri::command]
pub async fn template_chat(
    app: tauri::AppHandle,
    input: ChatTemplateInput,
) -> Result<AssistantMessage, String> {
    services::chat::template(&app, input)
        .await
        .map_err(command_error)
}

/// 更新已缓存会话的 stream options。
/// @param input stream options 请求。
#[tauri::command]
pub async fn set_chat_stream_options(input: ChatSetStreamOptionsInput) -> Result<(), String> {
    services::chat::set_stream_options(input)
        .await
        .map_err(command_error)
}

/// 更新已缓存会话的模型。
/// @param input 模型请求。
#[tauri::command]
pub async fn set_chat_model(input: ChatSetModelInput) -> Result<(), String> {
    services::chat::set_model(input)
        .await
        .map_err(command_error)
}

/// 更新已缓存会话的 thinking level。
/// @param input thinking level 请求。
#[tauri::command]
pub async fn set_chat_thinking_level(input: ChatSetThinkingLevelInput) -> Result<(), String> {
    services::chat::set_thinking_level(input)
        .await
        .map_err(command_error)
}

/// 更新已缓存会话的激活工具。
/// @param input 工具请求。
#[tauri::command]
pub async fn set_chat_tools(input: ChatSetToolsInput) -> Result<(), String> {
    services::chat::set_active_tools(input)
        .await
        .map_err(command_error)
}

/// 更新已缓存会话名称。
/// @param app Tauri 应用句柄。
/// @param input 会话名称请求。
#[tauri::command]
pub async fn set_chat_session_name(
    app: tauri::AppHandle,
    input: ChatSetSessionNameInput,
) -> Result<(), String> {
    services::chat::set_session_name(&app, input)
        .await
        .map_err(command_error)
}
