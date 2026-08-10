use std::{collections::HashMap, sync::Arc};

use ai::{
    agent::{
        harness::{
            compaction::{compact, DEFAULT_COMPACTION_SETTINGS},
            session::repo::SessionRepoRegistry,
            *,
        },
        AfterToolCallResult, AgentError, AgentMessage, BeforeToolCallResult, MHook, QueueMode,
    },
    model::{thinking_level_to_string, AssistantMessage, Model, ThinkingLevel},
};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use tauri::Emitter;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::{
    context,
    dto::{
        ChatAbortInput, ChatCompactInput, ChatCreateSessionInput, ChatDeleteSessionInput,
        ChatEditAndPromptUserMessageInput, ChatForkSessionInput, ChatListResourceNamesInput,
        ChatListSessionsInput, ChatOpenSessionInput, ChatPromptInput, ChatResolveToolApprovalInput,
        ChatResourceNameOutput, ChatSetModelInput, ChatSetSessionNameInput,
        ChatSetStreamOptionsInput, ChatSetThinkingLevelInput, ChatSetToolsInput, ChatSkillInput,
        ChatTemplateInput, ChatWithdrawTurnInput,
    },
    error::{AppError, AppResult},
    services,
};

/// 默认系统提示词。
const DEFAULT_SYSTEM_PROMPT: &str = "你是一个项目助手";

/// AgentHarness 前端事件名。
const CHAT_AGENT_HARNESS_EVENT: &str = "chat://agent-harness-event";

/// 工具审批请求前端事件名。
const CHAT_TOOL_APPROVAL_REQUESTED_EVENT: &str = "chat://tool-approval-requested";

/// 会话改名前端事件名。
const CHAT_SESSION_NAME_EVENT: &str = "chat://session-name-event";

/// 发送给前端的 AgentHarness 事件载荷。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatAgentHarnessEventPayload {
    /// 会话 id。
    session_id: String,
    /// AgentHarness 事件内容。
    event: Value,
    /// 事件时间戳。
    timestamp: u128,
}

/// 发送给前端的会话改名事件载荷。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatSessionNameEventPayload {
    /// 会话 id。
    session_id: String,
    /// 会话名称。
    name: String,
    /// 事件时间戳。
    timestamp: u128,
}

/// 发送给前端的工具审批请求。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatToolApprovalRequestedPayload {
    /// 审批请求 id。
    approval_id: String,
    /// 会话 id。
    session_id: String,
    /// 工具调用 id。
    tool_call_id: String,
    /// 工具名称。
    tool_name: String,
    /// 工具参数。
    args: Value,
    /// 事件时间戳。
    timestamp: u128,
}

/// 桌面端工具审批 hook。
struct ChatToolApprovalHook {
    /// Tauri 应用句柄。
    app: tauri::AppHandle,
    /// 会话 id。
    session_id: String,
}

/// 桥接 AgentHarness 事件到 Tauri 前端。
struct TauriAgentHarnessListener {
    /// Tauri 应用句柄。
    app: tauri::AppHandle,
    /// 会话 id。
    session_id: String,
}

/// 聊天 Harness 的动态系统提示词提供者。
struct ChatSystemPromptProvider;

/// 调用插件 Harness hook 并采用最后一个非空结果。
/// @param hook 插件协议中的 hook 名称。
/// @param event 已序列化的 Harness 事件快照。
async fn call_plugin_harness_hook<T>(hook: &str, event: Value) -> Result<Option<T>, AgentError>
where
    T: DeserializeOwned,
{
    let runtime = plugins::PluginRuntime::global()
        .map_err(|error| AgentError::Listener(error.to_string()))?;
    let results = runtime
        .call_harness_hook(hook, event)
        .await
        .map_err(|error| AgentError::Listener(error.to_string()))?;
    let mut final_result = None;
    for result in results {
        if !result.is_null() {
            final_result = Some(
                serde_json::from_value(result)
                    .map_err(|error| AgentError::Listener(error.to_string()))?,
            );
        }
    }
    Ok(final_result)
}

/// 插件 before-agent-start hook。
struct PluginBeforeAgentStartHook;

/// 插件 context hook。
struct PluginContextHook;

/// 插件 Provider 请求前 hook。
struct PluginBeforeProviderRequestHook;

/// 插件 Provider payload 前 hook。
struct PluginBeforeProviderPayloadHook;

/// 插件 Provider 响应后 hook。
struct PluginAfterProviderResponseHook;

/// 插件 tool call 前 hook。
struct PluginToolCallHook;

/// 插件 tool result hook。
struct PluginToolResultHook;

#[async_trait]
impl SystemPromptProvider for ChatSystemPromptProvider {
    /// 使用默认系统提示词和当前可自主调用的 Skill 生成系统提示词。
    async fn system_prompt<'a>(&'a self, context: SystemPromptContext<'a>) -> String {
        let skills_prompt = format_skills_for_system_prompt(&context.resources.skills);
        if skills_prompt.is_empty() {
            DEFAULT_SYSTEM_PROMPT.to_string()
        } else {
            format!("{DEFAULT_SYSTEM_PROMPT}\n\n{skills_prompt}")
        }
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessEvent<'event>, ()> for TauriAgentHarnessListener {
    /// 转发 AgentHarness 事件。
    /// @param event AgentHarness 事件。
    async fn execute(&self, event: &mut AgentHarnessEvent<'event>) -> Result<(), AgentError> {
        let event_value = serde_json::to_value(event).map_err(|error| {
            eprintln!("序列化 AgentHarness 事件失败: {error:?}");
            AgentError::Listener(error.to_string())
        })?;
        let plugin_notification = match plugins::PluginRuntime::global() {
            Ok(runtime) => runtime.notify_harness_event(event_value.clone()).await,
            Err(error) => Err(error),
        };
        if let Err(error) = plugin_notification {
            eprintln!("发送插件 Harness 事件失败: {error:?}");
            return Err(AgentError::Listener(error.to_string()));
        }
        let payload = ChatAgentHarnessEventPayload {
            session_id: self.session_id.clone(),
            event: event_value,
            timestamp: now_millis(),
        };
        self.app
            .emit(CHAT_AGENT_HARNESS_EVENT, payload)
            .map_err(|error| {
                eprintln!("发送 AgentHarness 前端事件失败: {error:?}");
                AgentError::Listener(error.to_string())
            })
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessOwnEvent<'event>, Option<BeforeToolCallResult>>
    for ChatToolApprovalHook
{
    /// 向客户端请求工具审批，并等待客户端结算。
    /// @param event 当前工具调用事件。
    async fn execute(
        &self,
        event: &mut AgentHarnessOwnEvent<'event>,
    ) -> Result<Option<BeforeToolCallResult>, AgentError> {
        let preference = services::models::get_preference(&self.app)
            .await
            .map_err(|error| {
                eprintln!("读取工具审批偏好失败: {error:?}");
                AgentError::Listener(error.to_string())
            })?;
        if preference.approval != 0 {
            return Ok(None);
        }
        let AgentHarnessOwnEvent::ToolCall {
            tool_call_id,
            tool_name,
            input,
        } = event
        else {
            return Ok(None);
        };
        let approval_id = Uuid::new_v4().to_string();
        let (sender, receiver) = oneshot::channel();
        context::context().tool_approvals.lock().await.insert(
            approval_id.clone(),
            context::ToolApprovalWaiter {
                session_id: self.session_id.clone(),
                sender,
            },
        );
        let payload = ChatToolApprovalRequestedPayload {
            approval_id: approval_id.clone(),
            session_id: self.session_id.clone(),
            tool_call_id: (*tool_call_id).to_string(),
            tool_name: (*tool_name).to_string(),
            args: Value::Object((*input).clone()),
            timestamp: now_millis(),
        };
        if let Err(error) = self.app.emit(CHAT_TOOL_APPROVAL_REQUESTED_EVENT, payload) {
            eprintln!("发送工具审批请求失败: {error:?}");
            context::context()
                .tool_approvals
                .lock()
                .await
                .remove(&approval_id);
            return Ok(Some(blocked_tool_approval("客户端不可用，已拒绝工具执行")));
        }
        let approved = receiver.await.unwrap_or(false);
        context::context()
            .tool_approvals
            .lock()
            .await
            .remove(&approval_id);
        Ok((!approved).then(|| blocked_tool_approval("用户拒绝了工具执行")))
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessOwnEvent<'event>, Option<BeforeAgentStartResult>>
    for PluginBeforeAgentStartHook
{
    /// 调用插件 before-agent-start hook。
    /// @param event 当前 Harness 事件。
    async fn execute(
        &self,
        event: &mut AgentHarnessOwnEvent<'event>,
    ) -> Result<Option<BeforeAgentStartResult>, AgentError> {
        call_plugin_harness_hook(
            "beforeAgentStart",
            serde_json::to_value(event).map_err(|error| AgentError::Listener(error.to_string()))?,
        )
        .await
    }
}

#[async_trait]
impl MHook<Vec<AgentMessage>, Option<ContextResult>> for PluginContextHook {
    /// 调用插件 context hook。
    /// @param messages 当前上下文消息。
    async fn execute(
        &self,
        messages: &mut Vec<AgentMessage>,
    ) -> Result<Option<ContextResult>, AgentError> {
        call_plugin_harness_hook(
            "context",
            serde_json::to_value(messages)
                .map_err(|error| AgentError::Listener(error.to_string()))?,
        )
        .await
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessOwnEvent<'event>, Option<AgentHarnessStreamOptions>>
    for PluginBeforeProviderRequestHook
{
    /// 调用插件 Provider 请求前 hook。
    /// @param event 当前 Harness 事件。
    async fn execute(
        &self,
        event: &mut AgentHarnessOwnEvent<'event>,
    ) -> Result<Option<AgentHarnessStreamOptions>, AgentError> {
        call_plugin_harness_hook(
            "beforeProviderRequest",
            serde_json::to_value(event).map_err(|error| AgentError::Listener(error.to_string()))?,
        )
        .await
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessOwnEvent<'event>, Option<Value>>
    for PluginBeforeProviderPayloadHook
{
    /// 调用插件 Provider payload 前 hook。
    /// @param event 当前 Harness 事件。
    async fn execute(
        &self,
        event: &mut AgentHarnessOwnEvent<'event>,
    ) -> Result<Option<Value>, AgentError> {
        call_plugin_harness_hook(
            "beforeProviderPayload",
            serde_json::to_value(event).map_err(|error| AgentError::Listener(error.to_string()))?,
        )
        .await
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessOwnEvent<'event>, ()> for PluginAfterProviderResponseHook {
    /// 调用插件 Provider 响应后 hook。
    /// @param event 当前 Harness 事件。
    async fn execute(&self, event: &mut AgentHarnessOwnEvent<'event>) -> Result<(), AgentError> {
        let _: Option<Value> = call_plugin_harness_hook(
            "afterProviderResponse",
            serde_json::to_value(event).map_err(|error| AgentError::Listener(error.to_string()))?,
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessOwnEvent<'event>, Option<BeforeToolCallResult>>
    for PluginToolCallHook
{
    /// 调用插件 tool call 前 hook。
    /// @param event 当前 Harness 事件。
    async fn execute(
        &self,
        event: &mut AgentHarnessOwnEvent<'event>,
    ) -> Result<Option<BeforeToolCallResult>, AgentError> {
        call_plugin_harness_hook(
            "toolCall",
            serde_json::to_value(event).map_err(|error| AgentError::Listener(error.to_string()))?,
        )
        .await
    }
}

#[async_trait]
impl<'event> MHook<AgentHarnessOwnEvent<'event>, Option<AfterToolCallResult>>
    for PluginToolResultHook
{
    /// 调用插件 tool result hook。
    /// @param event 当前 Harness 事件。
    async fn execute(
        &self,
        event: &mut AgentHarnessOwnEvent<'event>,
    ) -> Result<Option<AfterToolCallResult>, AgentError> {
        call_plugin_harness_hook(
            "toolResult",
            serde_json::to_value(event).map_err(|error| AgentError::Listener(error.to_string()))?,
        )
        .await
    }
}

/// 查询会话列表。
/// @param app Tauri 应用句柄。
/// @param input 会话列表请求。
pub async fn list_sessions(
    app: &tauri::AppHandle,
    input: ChatListSessionsInput,
) -> AppResult<Vec<SessionMetadata>> {
    let cwd = context::context_async(app)
        .await?
        .env
        .read()
        .await
        .cwd()
        .to_string();
    SessionRepoRegistry::global()
        .get(&input.storage_type)?
        .list(ListOptions { cwd: Some(cwd) })
        .await
        .map_err(Into::into)
}

/// 返回全部可用会话仓储名称。
pub fn list_repos() -> Vec<String> {
    SessionRepoRegistry::global().names()
}

/// 打开具体会话，构造 AgentHarness 并返回会话上下文。
/// @param app Tauri 应用句柄。
/// @param input 打开会话请求。
pub async fn open_session(
    app: &tauri::AppHandle,
    input: ChatOpenSessionInput,
) -> AppResult<SessionContext> {
    if let Some(session_context) = get_agent_harness_context(&input.metadata.id).await? {
        return Ok(session_context);
    }

    let session = SessionRepoRegistry::global()
        .get(&input.storage_type)?
        .open(input.metadata)
        .await?;
    let session_context = session.build_context_view().await;
    let model = match &session_context.model {
        Some(model_selection) => {
            match services::models::find_model_by_selection(app, model_selection).await {
                Ok(model) => model,
                Err(AppError::SessionModelNotFound { .. }) => {
                    services::models::find_any_model(app).await?
                }
                Err(error) => return Err(error),
            }
        }
        None => services::models::find_any_model(app).await?,
    };
    let thinking_level =
        services::models::string_to_thinking_level(&session_context.thinking_level)?;
    let session_id = session.with_metadata_guard().await.id.clone();
    let harness =
        build_agent_harness(app, &session_id, session.clone(), model, thinking_level).await?;
    put_agent_harness(&session_id, harness).await?;
    Ok(session_context)
}

/// 创建会话，构造 AgentHarness 并返回新会话元信息。
/// @param app Tauri 应用句柄。
/// @param input 创建会话请求。
pub async fn create_session(
    app: &tauri::AppHandle,
    input: ChatCreateSessionInput,
) -> AppResult<SessionMetadata> {
    let cwd = context::context_async(app)
        .await?
        .env
        .read()
        .await
        .cwd()
        .to_string();
    let session = SessionRepoRegistry::global()
        .get(&input.storage_type)?
        .create(CreateOptions {
            id: None,
            cwd,
            parent_session_path: None,
        })
        .await?;
    session
        .append_model_change(input.model.provider.clone(), input.model.id.clone())
        .await?;
    if let Some(thinking_level) = input.thinking_level {
        session
            .append_thinking_level_change(thinking_level_to_string(Some(&thinking_level)))
            .await?;
    }
    let metadata = session.get_metadata().await;
    let harness = build_agent_harness(
        app,
        &metadata.id,
        session.clone(),
        input.model,
        input.thinking_level,
    )
    .await?;
    put_agent_harness(&metadata.id, harness).await?;
    Ok(metadata)
}

/// 基于指定节点创建独立会话，并缓存新会话的 AgentHarness。
/// @param app Tauri 应用句柄。
/// @param input Fork 会话请求。
pub async fn fork_session(
    app: &tauri::AppHandle,
    input: ChatForkSessionInput,
) -> AppResult<SessionMetadata> {
    let source_harness = clone_agent_harness(&input.source_session_id)?;
    ensure_harness_idle(&source_harness).await?;
    source_harness.flush_pending_session_writes().await?;
    let entry = source_harness
        .session()
        .get_chat_entry(input.index)
        .await
        .ok_or_else(|| AppError::AiHarness(format!("聊天消息索引不存在: {}", input.index)))?;

    // 新会话沿源节点路径保留模型与思考等级配置。
    let turn_state = source_harness.create_turn_state().await;
    let session = SessionRepoRegistry::global()
        .get(&input.storage_type)?
        .fork(
            source_harness.session(),
            SessionForkOptions {
                entry_id: Some(entry.id().to_string()),
                position: Some(SessionForkPosition::Before),
                id: None,
            },
        )
        .await?;
    let metadata = session.get_metadata().await;
    let harness = build_agent_harness(
        app,
        &metadata.id,
        Arc::clone(&session),
        turn_state.model,
        turn_state.thinking_level,
    )
    .await?;
    put_agent_harness(&metadata.id, harness).await?;
    Ok(metadata)
}

/// 删除会话，并清理全局 Context 中的 AgentHarness。
/// @param input 删除会话请求。
pub async fn delete_session(input: ChatDeleteSessionInput) -> AppResult<()> {
    let session_id = input.metadata.id.clone();
    SessionRepoRegistry::global()
        .get(&input.storage_type)?
        .delete(input.metadata)
        .await?;
    remove_agent_harness(&session_id).await?;
    Ok(())
}

/// 对已缓存会话发起 prompt。
/// @param app Tauri 应用句柄。
/// @param input prompt 请求。
pub async fn prompt(
    app: &tauri::AppHandle,
    input: ChatPromptInput,
) -> AppResult<Option<AssistantMessage>> {
    let harness = clone_agent_harness(&input.session_id)?;
    let text = input.text;
    let is_idle = harness.is_idle().await;
    if !is_idle {
        harness.steer(text, input.images).await?;
        return Ok(None);
    }
    set_default_session_name(app, &harness, &input.session_id, prompt_session_name(&text)).await?;
    harness
        .prompt(text, input.images)
        .await
        .map(Some)
        .map_err(AppError::from)
}

/// 终止已缓存会话的当前 run。
/// @param input 终止请求。
pub async fn abort(input: ChatAbortInput) -> AppResult<()> {
    cancel_tool_approvals(&input.session_id).await;
    let harness = clone_agent_harness(&input.session_id)?;
    harness.abort().await.map(|_| ()).map_err(AppError::from)
}

/// 结算客户端工具审批请求。
/// @param input 审批结算请求。
pub async fn resolve_tool_approval(input: ChatResolveToolApprovalInput) -> AppResult<()> {
    let mut approvals = context::context().tool_approvals.lock().await;
    if let Some(waiter) = approvals.get(&input.approval_id) {
        if waiter.session_id != input.session_id {
            eprintln!("工具审批会话不匹配: approval_id={}", input.approval_id);
            return Err(AppError::AiHarness("工具审批会话不匹配".to_string()));
        }
    }
    let waiter = approvals.remove(&input.approval_id);
    let Some(waiter) = waiter else {
        return Err(AppError::AiHarness("工具审批请求已失效".to_string()));
    };
    let _ = waiter.sender.send(input.approved);
    Ok(())
}

/// 查询已缓存会话 Harness 中可调用的模板和 Skill 资源名称。
/// @param app Tauri 应用句柄。
/// @param input 资源名称查询请求。
/// @returns 第一个元素为模板，第二个元素为 Skill。
pub async fn list_resources_names(
    app: &tauri::AppHandle,
    input: ChatListResourceNamesInput,
) -> AppResult<Vec<Vec<ChatResourceNameOutput>>> {
    let harness = match clone_agent_harness(&input.session_id) {
        Ok(harness) => harness,
        Err(AppError::ChatAgentHarnessNotFound(_)) => return list_resources_from_files(app).await,
        Err(error) => return Err(error),
    };
    let resources = harness.get_resources().await;

    Ok(vec![
        resources
            .prompt_templates
            .iter()
            .map(|template| ChatResourceNameOutput {
                name: template.name.clone(),
                description: template
                    .description
                    .as_deref()
                    .unwrap_or_default()
                    .to_string(),
            })
            .collect(),
        resources
            .skills
            .iter()
            .map(|skill| ChatResourceNameOutput {
                name: skill.name.clone(),
                description: skill.description.clone(),
            })
            .collect(),
    ])
}

/// 从资源模块读取模板和 Skill，供尚未创建 Harness 的会话使用。
/// @param app Tauri 应用句柄。
async fn list_resources_from_files(
    app: &tauri::AppHandle,
) -> AppResult<Vec<Vec<ChatResourceNameOutput>>> {
    let template_files = services::resources::list_template_files(app).await?;
    let mut templates = Vec::with_capacity(template_files.len());

    for template_file in template_files {
        let template =
            services::resources::get_template_file(app, &template_file.name, &template_file.dir)
                .await?;
        templates.push(ChatResourceNameOutput {
            name: template.name,
            description: template.description,
        });
    }

    let skills = services::resources::list_skill_files(app)
        .await?
        .into_iter()
        .map(|skill| ChatResourceNameOutput {
            name: skill.name,
            description: skill.description,
        })
        .collect();

    Ok(vec![templates, skills])
}

/// 压缩已缓存会话的历史上下文。
/// @param input 会话压缩请求。
pub async fn compact_session(
    app: &tauri::AppHandle,
    input: ChatCompactInput,
) -> AppResult<ai::agent::harness::compaction::compaction::CompactionResult> {
    let harness = clone_agent_harness(&input.session_id)?;
    let auth_provider = services::models::auth_provider(app).await?;
    let model = harness.with_model().await;
    let auth = auth_provider
        .api_key_and_headers(&model)
        .await
        .unwrap_or_default();
    compact(
        harness.session(),
        &model,
        &auth,
        None,
        DEFAULT_COMPACTION_SETTINGS,
        input.custom_instructions.as_deref(),
    )
    .await
    .map_err(AppError::from)
}

/// 回撤一条用户消息及其后续活跃分支内容。
///
/// 此操作先中止运行中的 run 并等待待写入内容落盘，再切换 Session 的活跃 leaf；不删除被回撤分支，后续可使用保留的条目 id 导航回原分支。
/// @param input 回撤请求。
pub async fn withdraw_turn(input: ChatWithdrawTurnInput) -> AppResult<NavigateTreeResult> {
    let harness = clone_agent_harness(&input.session_id)?;
    harness.abort().await.map_err(AppError::from)?;
    harness.flush_pending_session_writes().await?;
    harness
        .navigate_tree(input.index, false, None, false, None)
        .await
        .map_err(AppError::from)
}

/// 回撤当前会话到用户消息之前，供客户端编辑后重新发送。
///
/// 编辑运行中的消息时，先中止当前 run 并等待其落盘，再按聊天索引回撤。
/// @param input 用户消息编辑与发送请求。
pub async fn edit_and_prompt_user_message(
    input: ChatEditAndPromptUserMessageInput,
) -> AppResult<()> {
    let harness = clone_agent_harness(&input.session_id)?;
    harness.abort().await.map_err(AppError::from)?;
    harness.flush_pending_session_writes().await?;
    harness
        .navigate_tree(input.index, false, None, false, None)
        .await?;
    Ok(())
}

/// 对已缓存会话发起 skill。
/// @param app Tauri 应用句柄。
/// @param input skill 请求。
pub async fn skill(app: &tauri::AppHandle, input: ChatSkillInput) -> AppResult<AssistantMessage> {
    let harness = clone_agent_harness(&input.session_id)?;
    set_default_session_name(app, &harness, &input.session_id, skill_session_name(&input)).await?;
    harness
        .skill(&input.name, input.additional_instructions.as_deref())
        .await
        .map_err(AppError::from)
}

/// 对已缓存会话发起 prompt template。
/// @param app Tauri 应用句柄。
/// @param input prompt template 请求。
pub async fn template(
    app: &tauri::AppHandle,
    input: ChatTemplateInput,
) -> AppResult<AssistantMessage> {
    let harness = clone_agent_harness(&input.session_id)?;
    set_default_session_name(
        app,
        &harness,
        &input.session_id,
        template_session_name(&input),
    )
    .await?;
    harness
        .prompt_from_template(&input.name, &input.args)
        .await
        .map_err(AppError::from)
}

/// 更新已缓存会话的 stream options。
/// @param input stream options 请求。
pub async fn set_stream_options(input: ChatSetStreamOptionsInput) -> AppResult<()> {
    let harness = clone_agent_harness(&input.session_id)?;
    harness
        .set_stream_options(
            input.stream_options,
            vec![Box::new(PluginBeforeProviderPayloadHook)],
            vec![Box::new(PluginAfterProviderResponseHook)],
        )
        .await;
    Ok(())
}

/// 更新已缓存会话的模型。
/// @param input 模型请求。
pub async fn set_model(input: ChatSetModelInput) -> AppResult<()> {
    let harness = clone_agent_harness(&input.session_id)?;
    harness.set_model(input.model).await.map_err(AppError::from)
}

/// 更新已缓存会话的 thinking level。
/// @param input thinking level 请求。
pub async fn set_thinking_level(input: ChatSetThinkingLevelInput) -> AppResult<()> {
    let harness = clone_agent_harness(&input.session_id)?;
    harness
        .set_thinking_level(input.thinking_level)
        .await
        .map_err(AppError::from)
}

/// 重置已缓存会话的工具注册表和激活工具。
/// @param app Tauri 应用句柄。
/// @param input 工具请求。
pub async fn set_tools(input: ChatSetToolsInput) -> AppResult<()> {
    let active_tool_names = active_tool_names(&input.tools);
    for tool_name in &active_tool_names {
        services::models::validate_tool_name(tool_name)?;
    }
    let harness = clone_agent_harness(&input.session_id)?;
    let tools = tool::ToolRegistry::global().tools();
    harness
        .set_tools(tools, Some(active_tool_names))
        .await
        .map_err(AppError::from)
}

/// 更新已缓存会话的激活工具。
/// @param input 工具请求。
pub async fn set_active_tools(input: ChatSetToolsInput) -> AppResult<()> {
    let active_tool_names = active_tool_names(&input.tools);
    for tool_name in &active_tool_names {
        services::models::validate_tool_name(tool_name)?;
    }
    let harness = clone_agent_harness(&input.session_id)?;
    harness
        .set_active_tools(active_tool_names)
        .await
        .map_err(AppError::from)
}

/// 更新已缓存会话名称并通知前端。
/// @param app Tauri 应用句柄。
/// @param input 会话名称请求。
pub async fn set_session_name(
    app: &tauri::AppHandle,
    input: ChatSetSessionNameInput,
) -> AppResult<()> {
    let harness = clone_agent_harness(&input.session_id)?;
    set_session_name_for_harness(app, &harness, &input.session_id, &input.name).await
}

/// 当前毫秒时间戳。
fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

/// 截取文本前 N 个字符。
/// @param text 原始文本。
/// @param max_chars 最大字符数。
fn take_chars(text: &str, max_chars: usize) -> String {
    text.trim().chars().take(max_chars).collect()
}

/// 解析工具配置并返回实际激活的工具名称。
/// @param tools 工具配置数组，首元素为启用状态（0/1）。
fn active_tool_names(tools: &[String]) -> Vec<String> {
    let has_search = tools.iter().skip(1).any(|tool_name| tool_name == "search");
    if tools.first().is_none_or(|enabled| enabled != "1") {
        return has_search
            .then(|| vec!["fetch".to_string(), "search".to_string()])
            .unwrap_or_default();
    }

    let mut active_tool_names = tools.iter().skip(1).cloned().collect::<Vec<_>>();
    if has_search
        && !active_tool_names
            .iter()
            .any(|tool_name| tool_name == "fetch")
    {
        active_tool_names.push("fetch".to_string());
    }
    active_tool_names
}

/// 生成 prompt 默认会话名。
/// @param text prompt 文本。
fn prompt_session_name(text: &str) -> Option<String> {
    let name = take_chars(text, 15);
    (!name.is_empty()).then(|| format!("{name}..."))
}

/// 生成 skill 默认会话名。
/// @param input skill 请求。
fn skill_session_name(input: &ChatSkillInput) -> Option<String> {
    let instructions = input
        .additional_instructions
        .as_deref()
        .map(|value| take_chars(value, 10))
        .filter(|value| !value.is_empty());
    let name = match instructions {
        Some(instructions) => format!("技能:{},{}...", input.name, instructions),
        None => format!("技能:{}...", input.name),
    };
    Some(name)
}

/// 生成 prompt template 默认会话名。
/// @param input prompt template 请求。
fn template_session_name(input: &ChatTemplateInput) -> Option<String> {
    let args = input
        .args
        .iter()
        .take(3)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",");
    Some(format!("模版:{},参数:{}...", input.name, args))
}

/// 当当前名称为空时设置默认会话名称。
/// @param app Tauri 应用句柄。
/// @param harness AgentHarness 共享句柄。
/// @param session_id 会话 id。
/// @param name 默认名称。
async fn set_default_session_name(
    app: &tauri::AppHandle,
    harness: &AgentHarness,
    session_id: &str,
    name: Option<String>,
) -> AppResult<()> {
    let Some(name) = name else { return Ok(()) };
    if harness
        .session()
        .with_metadata_guard()
        .await
        .name
        .trim()
        .is_empty()
    {
        set_session_name_for_harness(app, harness, session_id, &name).await?;
    }
    Ok(())
}

/// 设置会话名称并发送后端改名事件。
/// @param app Tauri 应用句柄。
/// @param harness AgentHarness 共享句柄。
/// @param session_id 会话 id。
/// @param name 会话名称。
async fn set_session_name_for_harness(
    app: &tauri::AppHandle,
    harness: &AgentHarness,
    session_id: &str,
    name: &str,
) -> AppResult<()> {
    harness.append_session_name(name).await?;
    app.emit(
        CHAT_SESSION_NAME_EVENT,
        ChatSessionNameEventPayload {
            session_id: session_id.to_string(),
            name: name.to_string(),
            timestamp: now_millis(),
        },
    )
    .map_err(AppError::from)
}

/// 构造 AgentHarness。
/// @param app Tauri 应用句柄。
/// @param session_id 会话 id。
/// @param session 会话句柄。
/// @param model 当前模型。
async fn build_agent_harness(
    app: &tauri::AppHandle,
    session_id: &str,
    session: SessionHandle,
    model: Model,
    thinking_level: Option<ThinkingLevel>,
) -> AppResult<AgentHarness> {
    let tools = tool::ToolRegistry::global().tools();
    let preference = services::models::get_preference(app).await?;
    let active_tool_names = active_tool_names(&preference.tools.0);
    let env = context::context_async(app).await?.env.read().await.clone();
    let template_paths =
        services::resources::resource_dirs(app, services::resources::TEMPLATES_DIR_NAME)
            .await?
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>();
    let skill_paths = services::resources::resource_dirs(app, services::resources::SKILLS_DIR_NAME)
        .await?
        .into_iter()
        .map(|(path, _)| path)
        .collect::<Vec<_>>();
    let (prompt_templates, _) = load_prompt_templates(env.as_ref(), &template_paths).await;
    let (skills, _) = load_skills(env.as_ref(), &skill_paths).await;
    AgentHarness::new(AgentHarnessOptions {
        env,
        session,
        model,
        thinking_level,
        tools,
        active_tool_names: Some(active_tool_names),
        resources: AgentHarnessResources {
            prompt_templates,
            skills,
        },
        stream_options: AgentHarnessStreamOptions::default(),
        system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
        system_prompt_provider: Some(Arc::new(ChatSystemPromptProvider)),
        auth_provider: Some(services::models::auth_provider(app).await?),
        steering_mode: QueueMode::OneAtATime,
        follow_up_mode: QueueMode::OneAtATime,
        listeners: vec![Arc::new(TauriAgentHarnessListener {
            app: app.clone(),
            session_id: session_id.to_string(),
        })],
        before_agent_start_hooks: vec![Box::new(PluginBeforeAgentStartHook)],
        context_hooks: vec![Box::new(PluginContextHook)],
        before_provider_request_hooks: vec![Box::new(PluginBeforeProviderRequestHook)],
        before_provider_payload_hooks: vec![Box::new(PluginBeforeProviderPayloadHook)],
        after_provider_response_hooks: vec![Box::new(PluginAfterProviderResponseHook)],
        tool_call_hooks: vec![
            Box::new(PluginToolCallHook),
            Box::new(ChatToolApprovalHook {
                app: app.clone(),
                session_id: session_id.to_string(),
            }),
        ],
        tool_result_hooks: vec![Box::new(PluginToolResultHook)],
    })
    .await
    .map_err(Into::into)
}

/// 确认 Harness 处于空闲状态，避免从未落盘的运行中会话分支创建副本。
/// @param harness 源会话 Harness。
async fn ensure_harness_idle(harness: &AgentHarness) -> AppResult<()> {
    if harness.is_idle().await {
        Ok(())
    } else {
        Err(AppError::AiHarness(
            "Fork 会话要求源会话处于空闲状态".to_string(),
        ))
    }
}

/// 从全局 AgentHarness 映射读取会话上下文。
/// @param session_id 会话 id。
async fn get_agent_harness_context(session_id: &str) -> AppResult<Option<SessionContext>> {
    let agent_harness = get_agent_harness(session_id, Arc::clone)?;
    match agent_harness {
        Some(harness) => Ok(Some(harness.session().build_context_view().await)),
        None => Ok(None),
    }
}

/// 在闭包内读取全局 AgentHarness 共享句柄。
/// @param session_id 会话 id。
/// @param read 共享句柄读取回调。
fn get_agent_harness<R>(
    session_id: &str,
    read: impl FnOnce(&Arc<AgentHarness>) -> R,
) -> AppResult<Option<R>> {
    let agent_harnesses = context::context()
        .agent_harnesses
        .lock()
        .map_err(|error| AppError::ContextLock(error.to_string()))?;
    Ok(agent_harnesses.get(session_id).map(read))
}

/// 从全局 AgentHarness 映射克隆会话 Harness 共享句柄，避免持锁 await。
/// @param session_id 会话 id。
fn clone_agent_harness(session_id: &str) -> AppResult<Arc<AgentHarness>> {
    let agent_harnesses = context::context()
        .agent_harnesses
        .lock()
        .map_err(|error| AppError::ContextLock(error.to_string()))?;
    agent_harnesses
        .get(session_id)
        .map(Arc::clone)
        .ok_or_else(|| AppError::ChatAgentHarnessNotFound(session_id.to_string()))
}

/// 写入全局 AgentHarness 映射。
/// @param session_id 会话 id。
/// @param harness AgentHarness 实例。
async fn put_agent_harness(session_id: &str, harness: AgentHarness) -> AppResult<()> {
    let current_agent_harnesses = {
        let mut agent_harnesses = context::context()
            .agent_harnesses
            .lock()
            .map_err(|error| AppError::ContextLock(error.to_string()))?;
        std::mem::take(&mut *agent_harnesses)
    };
    let mut next_agent_harnesses = HashMap::new();
    for (current_session_id, current) in current_agent_harnesses {
        if !current.is_idle().await {
            next_agent_harnesses.insert(current_session_id, current);
        }
    }
    next_agent_harnesses.insert(session_id.to_string(), Arc::new(harness));
    let mut agent_harnesses = context::context()
        .agent_harnesses
        .lock()
        .map_err(|error| AppError::ContextLock(error.to_string()))?;
    *agent_harnesses = next_agent_harnesses;
    Ok(())
}

/// 移除全局 AgentHarness 映射。
/// @param session_id 会话 id。
async fn remove_agent_harness(session_id: &str) -> AppResult<()> {
    cancel_tool_approvals(session_id).await;
    let agent_harness = {
        let mut agent_harnesses = context::context()
            .agent_harnesses
            .lock()
            .map_err(|error| AppError::ContextLock(error.to_string()))?;
        agent_harnesses.remove(session_id)
    };
    if let Some(agent_harness) = agent_harness {
        if !agent_harness.is_idle().await {
            let mut agent_harnesses = context::context()
                .agent_harnesses
                .lock()
                .map_err(|error| AppError::ContextLock(error.to_string()))?;
            agent_harnesses.insert(session_id.to_string(), agent_harness);
        }
    }
    Ok(())
}

/// 构造被拒绝工具调用的 hook 返回值。
/// @param reason 拒绝原因。
fn blocked_tool_approval(reason: &str) -> BeforeToolCallResult {
    BeforeToolCallResult {
        block: Some(true),
        reason: Some(reason.to_string()),
    }
}

/// 取消指定会话全部待结算的工具审批请求。
/// @param session_id 会话 id。
async fn cancel_tool_approvals(session_id: &str) {
    let approval_ids = context::context()
        .tool_approvals
        .lock()
        .await
        .iter()
        .filter_map(|(approval_id, waiter)| {
            (waiter.session_id == session_id).then(|| approval_id.clone())
        })
        .collect::<Vec<_>>();
    let mut approvals = context::context().tool_approvals.lock().await;
    for approval_id in approval_ids {
        if let Some(waiter) = approvals.remove(&approval_id) {
            let _ = waiter.sender.send(false);
        }
    }
}
