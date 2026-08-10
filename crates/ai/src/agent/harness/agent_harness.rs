//! Harness 框架的核心实现：在底层 `Agent` 之上叠加资源管理、会话持久化、上下文压缩、
//! 分支总结、队列协同与保存点等能力。

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};

use crate::{
    agent::{
        agent::{Agent, AgentHandle, AgentOptions, QueueMode},
        env::ExecutionEnv,
        harness::{
            compaction::branch_summarization::{
                collect_entries_for_branch_summary, generate_branch_summary, GenerateBranchSummaryOptions,
            },
            prompt_templates::format_prompt_template_invocation,
            session::{BranchMoveSummary, Session, SessionHandle},
            skills::format_skill_invocation,
            types::*,
            HarnessResult,
        },
        types::{
            AfterToolCallContext, AfterToolCallResult, AgentError, AgentEventListener, AgentMessage, AgentTool,
            BeforeToolCallContext, BeforeToolCallResult, MEHook, MHook, PrepareNextTurnContext,
            ShouldStopAfterTurnContext, StreamFn,
        },
    },
    model::{
        stream::stream_simple,
        types::{
            thinking_level_to_string, AssistantMessage, Auth, AuthProvider, ContentBlock, Context, ImageContent, Model,
            ProviderPayloadCallback, ProviderResponse, ProviderResponseCallback, StreamError, StreamOptions,
            TextContent, ThinkingLevel, UserContent, UserMessage,
        },
    },
};

use crate::agent::types::AgentEvent;

/// Harness 侧 Provider stream 适配器。
struct HarnessStreamFn {
    /// Provider 请求前 hooks。
    before_provider_request_hooks: Vec<Box<BeforeProviderRequestHook>>,
    /// Harness 事件监听器。
    stream_listeners: Vec<Arc<AgentHarnessListener>>,
}

#[async_trait]
impl StreamFn for HarnessStreamFn {
    /// 注入认证、请求前 hook 与 Harness 事件后调用实际模型 stream。
    async fn stream<'a>(
        &'a self,
        model: &'a Model,
        context: Context,
        options: &'a StreamOptions,
        auth: &'a Auth,
        sink: &mut dyn crate::model::api_registry::AssistantMessageEventSink,
    ) -> Result<AssistantMessage, AgentError> {
        let mut options = options.clone();
        let request_event = AgentHarnessOwnEvent::BeforeProviderRequest { model, stream_options: &options };
        emit_listeners(&self.stream_listeners, AgentHarnessEvent::Harness(&request_event)).await?;
        for hook in &self.before_provider_request_hooks {
            let patch = {
                let mut request_event = AgentHarnessOwnEvent::BeforeProviderRequest { model, stream_options: &options };
                hook.execute(&mut request_event).await?
            };
            apply_stream_options_patch(&mut options, patch);
        }

        stream_simple(model, context, &options, auth, sink).await.map_err(AgentError::from)
    }
}

/// 把一段文本、文本签名与可选图像内容打包为 `UserMessage`。
///
/// @param text 用户文本。
/// @param text_signature 文本块签名。
/// @param images 可选图像内容。
pub fn create_user_message(
    text: String,
    text_signature: Option<String>,
    images: Option<Vec<ImageContent>>,
) -> AgentMessage {
    let mut content = vec![ContentBlock::Text(TextContent { text, text_signature })];
    if let Some(images) = images {
        content.extend(images.into_iter().map(ContentBlock::Image));
    }
    AgentMessage::User(UserMessage {
        content: UserContent::Blocks(content),
        timestamp: crate::model::types::now_millis(),
    })
}

/// 把 Harness hook 错误转为 stream 错误。
fn stream_error(error: AgentError) -> StreamError {
    StreamError::Callback(error.to_string())
}

/// 把 headers 合并为一个 record；后者覆盖前者。
pub fn merge_headers(
    mut base: HashMap<String, String>,
    extra: Option<HashMap<String, String>>,
) -> HashMap<String, String> {
    if let Some(extra) = extra {
        base.extend(extra);
    }
    base
}

/// 把 stream options patch 按字段合并到 base 上。
pub fn apply_stream_options_patch(base: &mut StreamOptions, patch: Option<AgentHarnessStreamOptions>) {
    let Some(patch) = patch else {
        return;
    };

    if let Some(transport) = patch.transport {
        base.transport = Some(transport);
    }
    if let Some(cache_retention) = patch.cache_retention {
        base.cache_retention = Some(cache_retention);
    }
    if let Some(timeout_ms) = patch.timeout_ms {
        base.timeout_ms = Some(timeout_ms);
    }
    if let Some(max_retries) = patch.max_retries {
        base.max_retries = Some(max_retries);
    }
    if let Some(max_retry_delay_ms) = patch.max_retry_delay_ms {
        base.max_retry_delay_ms = Some(max_retry_delay_ms);
    }
    base.headers.extend(patch.headers);
    base.metadata.extend(patch.metadata);
}

/// 单轮 turn 的快照状态。
pub struct AgentHarnessTurnState {
    /// 本轮还原出的 AgentMessage 序列。
    pub messages: Vec<AgentMessage>,
    /// Provider session id。
    pub session_id: String,
    /// 本轮使用的 Model。
    pub model: Model,
    /// 本轮使用的 thinking level。
    pub thinking_level: Option<ThinkingLevel>,
    /// 当前 Harness 注册的全部工具。
    pub tools: Vec<Arc<dyn AgentTool>>,
    /// 本轮真正暴露给 Agent 的工具列表。
    pub active_tools: Vec<Arc<dyn AgentTool>>,
}

/// 系统提示词回调上下文。
pub struct SystemPromptContext<'a> {
    /// 执行环境。
    pub env: &'a dyn ExecutionEnv,
    /// 当前会话。
    pub session: &'a Session,
    /// 当前模型。
    pub model: &'a Model,
    /// 当前 thinking level。
    pub thinking_level: Option<&'a ThinkingLevel>,
    /// 当前激活工具。
    pub active_tools: &'a [Arc<dyn AgentTool>],
    /// 当前资源。
    pub resources: &'a AgentHarnessResources,
}

/// 系统提示词提供者。
#[async_trait]
pub trait SystemPromptProvider: Send + Sync {
    /// 返回当前 turn 的系统提示词。
    async fn system_prompt<'a>(&'a self, context: SystemPromptContext<'a>) -> String;
}

/// AgentHarness 构造选项。
pub struct AgentHarnessOptions {
    /// 执行环境。
    pub env: Arc<dyn ExecutionEnv>,
    /// 当前会话。
    pub session: SessionHandle,
    /// 当前模型。
    pub model: Model,
    /// 当前 thinking level。
    pub thinking_level: Option<ThinkingLevel>,
    /// 初始工具集合。
    pub tools: Vec<Arc<dyn AgentTool>>,
    /// 当前激活工具名。
    pub active_tool_names: Option<Vec<String>>,
    /// 初始资源。
    pub resources: AgentHarnessResources,
    /// Provider 请求选项。
    pub stream_options: AgentHarnessStreamOptions,
    /// 固定系统提示词。
    pub system_prompt: String,
    /// 动态系统提示词提供者。
    pub system_prompt_provider: Option<Arc<dyn SystemPromptProvider>>,
    /// Provider 认证提供者。
    pub auth_provider: Option<Arc<dyn AuthProvider>>,
    /// steering queue 模式。
    pub steering_mode: QueueMode,
    /// follow-up queue 模式。
    pub follow_up_mode: QueueMode,
    /// 任意事件监听器。
    pub listeners: Vec<Arc<AgentHarnessListener>>,
    /// `before_agent_start` hook 列表。
    pub before_agent_start_hooks: Vec<Box<BeforeAgentStartHook>>,
    /// context hook 列表。
    pub context_hooks: Vec<Box<ContextHook>>,
    /// Provider 请求前 hook 列表。
    pub before_provider_request_hooks: Vec<Box<BeforeProviderRequestHook>>,
    /// Provider payload hook 列表。
    pub before_provider_payload_hooks: Vec<Box<BeforeProviderPayloadHook>>,
    /// Provider 响应后 hook 列表。
    pub after_provider_response_hooks: Vec<Box<AfterProviderResponseHook>>,
    /// tool call hook 列表。
    pub tool_call_hooks: Vec<Box<ToolCallHook>>,
    /// tool result hook 列表。
    pub tool_result_hooks: Vec<Box<ToolResultHook>>,
}

/// 中止结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AbortResult {
    /// 被清空的转向消息队列。
    pub cleared_steer: Vec<AgentMessage>,
    /// 被清空的跟进消息队列。
    pub cleared_follow_up: Vec<AgentMessage>,
}

/// 树导航结果。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NavigateTreeResult {
    /// 是否被取消。
    pub cancelled: bool,
    /// 可回填到编辑器的文本。
    pub editor_text: Option<String>,
    /// 新写入的 summary entry。
    pub summary_entry: Option<SessionTreeEntry>,
}

/// 待写入会话存储的条目 patch。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PendingSessionWrite {
    /// 消息条目。
    Message { message: AgentMessage },
    /// model change 条目。
    ModelChange { provider: String, model_id: String },
    /// thinking level change 条目。
    ThinkingLevelChange { thinking_level: String },
    /// custom 条目。
    Custom { custom_type: String, data: Option<Value> },
    /// custom_message 条目。
    CustomMessage { custom_type: String, content: CustomMessageContent, display: bool, details: Option<Value> },
    /// label 条目。
    Label { target_id: String, label: Option<String> },
    /// session_info 条目。
    SessionInfo { name: Option<String> },
}

/// Harness provider payload callback adapter。
struct HarnessProviderPayloadCallback {
    /// 事件监听器。
    listeners: Vec<Arc<AgentHarnessListener>>,
    /// payload hooks。
    hooks: Vec<Box<BeforeProviderPayloadHook>>,
}

#[async_trait]
impl ProviderPayloadCallback for HarnessProviderPayloadCallback {
    /// 按注册顺序允许 hooks 整体替换 Provider payload。
    async fn on_payload(&self, payload: Value, model: &Model) -> Result<Value, StreamError> {
        let mut current = payload;
        for hook in &self.hooks {
            let event = AgentHarnessOwnEvent::BeforeProviderPayload { model, payload: &current };
            emit_listeners(&self.listeners, AgentHarnessEvent::Harness(&event)).await.map_err(stream_error)?;
            let mut event = AgentHarnessOwnEvent::BeforeProviderPayload { model, payload: &current };
            if let Some(next_payload) = hook.execute(&mut event).await.map_err(stream_error)? {
                current = next_payload;
            }
        }
        Ok(current)
    }
}

/// Harness provider response callback adapter。
struct HarnessProviderResponseCallback {
    /// 事件监听器。
    listeners: Vec<Arc<AgentHarnessListener>>,
    /// response hooks。
    hooks: Vec<Box<AfterProviderResponseHook>>,
}

#[async_trait]
impl ProviderResponseCallback for HarnessProviderResponseCallback {
    /// 转发真实 Provider HTTP 响应。
    async fn on_response(&self, response: ProviderResponse, _model: &Model) -> Result<ProviderResponse, StreamError> {
        let event = AgentHarnessOwnEvent::AfterProviderResponse { status: response.status, headers: &response.headers };
        emit_listeners(&self.listeners, AgentHarnessEvent::Harness(&event)).await.map_err(stream_error)?;
        for hook in &self.hooks {
            let mut event =
                AgentHarnessOwnEvent::AfterProviderResponse { status: response.status, headers: &response.headers };
            hook.execute(&mut event).await.map_err(stream_error)?;
        }
        Ok(response)
    }
}

/// 给 stream options 注入 Harness 回调。
pub fn with_stream_callbacks(
    options: AgentHarnessStreamOptions,
    thinking_level: Option<ThinkingLevel>,
    listeners: &Vec<Arc<AgentHarnessListener>>,
    before: Vec<Box<BeforeProviderPayloadHook>>,
    after: Vec<Box<AfterProviderResponseHook>>,
) -> StreamOptions {
    let mut stream_options = StreamOptions {
        transport: options.transport,
        timeout_ms: options.timeout_ms,
        max_retries: options.max_retries,
        max_retry_delay_ms: options.max_retry_delay_ms,
        headers: options.headers,
        metadata: options.metadata,
        cache_retention: options.cache_retention,
        reasoning: thinking_level,
        ..Default::default()
    };
    stream_options.on_payload =
        Some(Arc::new(HarnessProviderPayloadCallback { listeners: listeners.clone(), hooks: before }));
    stream_options.on_response =
        Some(Arc::new(HarnessProviderResponseCallback { listeners: listeners.clone(), hooks: after }));
    stream_options
}

/// Harness 框架对外暴露的核心类。
pub struct AgentHarness {
    /// 底层 Agent 实例。
    pub agent: Mutex<Agent>,
    /// 底层 Agent 的共享运行控制句柄。
    handle: Arc<AgentHandle>,
    /// 当前会话。
    session: SessionHandle,
    /// 当前生效的 Model。
    model: Arc<RwLock<Model>>,
    /// 当前生效的 thinking level。
    thinking_level: Arc<RwLock<Option<ThinkingLevel>>>,
    /// 当前会话激活的工具名列表。
    active_tool_names: Arc<RwLock<Vec<String>>>,
    /// 下一轮注入队列。
    next_turn_queue: RwLock<Vec<AgentMessage>>,
    /// 下一轮队列长度。
    next_turn_count: RwLock<usize>,
    /// 待写入会话存储的条目。
    pending_session_writes: RwLock<Vec<PendingSessionWrite>>,
    /// Harness 当前阶段。
    phase: RwLock<AgentHarnessPhase>,
    /// Harness 当前持有的资源。
    resources: Arc<RwLock<AgentHarnessResources>>,
    /// 动态系统提示词提供者。
    system_prompt_provider: Option<Arc<dyn SystemPromptProvider>>,
    /// 工具注册表。
    tools: Arc<RwLock<HashMap<String, Arc<dyn AgentTool>>>>,
    /// 任意事件监听器。
    listeners: Vec<Arc<AgentHarnessListener>>,
    /// `before_agent_start` hook 列表。
    before_agent_start_hooks: Vec<Box<BeforeAgentStartHook>>,
}

impl AgentHarness {
    /// 构造一个 `AgentHarness`。
    pub async fn new(options: AgentHarnessOptions) -> AgentHarnessRuntimeResult<Self> {
        let env = options.env;
        let stream_fn: Box<dyn StreamFn> = Box::new(HarnessStreamFn {
            before_provider_request_hooks: options.before_provider_request_hooks,
            stream_listeners: options.listeners.clone(),
        });
        let tools_by_name =
            options.tools.into_iter().map(|tool| (tool.definition().name, tool)).collect::<HashMap<_, _>>();
        let active_tool_names = options.active_tool_names.unwrap_or_else(|| tools_by_name.keys().cloned().collect());
        let model = Arc::new(RwLock::new(options.model));
        let thinking_level = Arc::new(RwLock::new(options.thinking_level));
        let tools = Arc::new(RwLock::new(tools_by_name));
        let active_tool_names = Arc::new(RwLock::new(active_tool_names));
        let resources = Arc::new(RwLock::new(options.resources));
        let mut initial_state = crate::agent::types::AgentState::default();
        initial_state.system_prompt = options.system_prompt;
        initial_state.tools = {
            let tools_guard = tools.read().await;
            let active_tool_names_guard = active_tool_names.read().await;
            active_tool_names_guard.iter().filter_map(|name| tools_guard.get(name).cloned()).collect()
        };
        let stream_options = with_stream_callbacks(
            options.stream_options,
            *thinking_level.read().await,
            &options.listeners,
            options.before_provider_payload_hooks,
            options.after_provider_response_hooks,
        );
        let agent = Agent::new(AgentOptions {
            env: Some(env),
            model: model.read().await.clone(),
            initial_state: Some(initial_state),
            stream_fn: Some(stream_fn),
            steering_mode: options.steering_mode,
            follow_up_mode: options.follow_up_mode,
            stream_options,
            auth_provider: options.auth_provider,
            tool_execution: Default::default(),
            transform_context: Some(Box::new(HarnessContextHook {
                listeners: options.listeners.clone(),
                hooks: options.context_hooks,
            })),
            before_tool_call: Some(Box::new(HarnessBeforeToolCallHook {
                listeners: options.listeners.clone(),
                hooks: options.tool_call_hooks,
            })),
            update_tool_call: None,
            after_tool_call: Some(Box::new(HarnessAfterToolCallHook {
                listeners: options.listeners.clone(),
                hooks: options.tool_result_hooks,
            })),
            prepare_next_turn: Some(Box::new(HarnessPrepareNextTurnHook {
                session: Arc::clone(&options.session),
                model: Arc::clone(&model),
                thinking_level: Arc::clone(&thinking_level),
                tools: Arc::clone(&tools),
                active_tool_names: Arc::clone(&active_tool_names),
                resources: Arc::clone(&resources),
                system_prompt_provider: options.system_prompt_provider.clone(),
            })),
            should_stop_after_turn: Some(Box::new(HarnessShouldStopAfterTurnHook {})),
        })?;
        let handle = agent.handle();
        Ok(Self {
            agent: Mutex::new(agent),
            handle,
            session: options.session,
            model,
            thinking_level,
            active_tool_names,
            next_turn_queue: RwLock::new(Vec::new()),
            pending_session_writes: RwLock::new(Vec::new()),
            phase: RwLock::new(AgentHarnessPhase::Idle),
            next_turn_count: RwLock::new(0),
            resources,
            system_prompt_provider: options.system_prompt_provider,
            tools,
            listeners: options.listeners,
            before_agent_start_hooks: options.before_agent_start_hooks,
        })
    }

    /// 读取当前阶段快照。
    pub async fn phase(&self) -> AgentHarnessPhase {
        *self.phase.read().await
    }

    /// 判断当前是否空闲。
    pub async fn is_idle(&self) -> bool {
        *self.phase.read().await == AgentHarnessPhase::Idle
    }

    /// 写入当前阶段。
    async fn set_phase(&self, phase: AgentHarnessPhase) {
        *self.phase.write().await = phase;
    }

    /// 订阅 Harness 任意事件。
    pub fn subscribe(&mut self, listener: Arc<AgentHarnessListener>) {
        self.listeners.push(listener);
    }

    /// 发布 Harness 自身事件。
    async fn emit_own(&self, event: AgentHarnessOwnEvent<'_>) -> Result<(), AgentError> {
        emit_listeners(&self.listeners, AgentHarnessEvent::Harness(&event)).await
    }

    /// 发布队列更新事件。
    async fn emit_queue_update(&self) -> Result<(), AgentError> {
        let steer = self.handle.get_steering_queue().await;
        let follow_up = self.handle.get_follow_up_queue().await;
        let next_turn = self.next_turn_queue.read().await;
        self.emit_own(AgentHarnessOwnEvent::QueueUpdate { steer: &steer, follow_up: &follow_up, next_turn: &next_turn })
            .await
    }

    /// 返回当前会话引用。
    pub fn session(&self) -> &Session {
        self.session.as_ref()
    }

    /// 校验工具名全部存在于注册表。
    async fn validate_tool_names(&self, tool_names: &[String]) -> AgentHarnessRuntimeResult<()> {
        let tools = self.tools.read().await;
        let missing = tool_names.iter().filter(|name| !tools.contains_key(*name)).cloned().collect::<Vec<_>>();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(AgentHarnessError::Message(format!("Unknown tool(s): {}", missing.join(", "))))
        }
    }

    /// 计算下一轮 turn 的快照状态。
    pub async fn create_turn_state(&self) -> AgentHarnessTurnState {
        let context = self.session.build_context().await;
        let session_id = self.session.with_metadata_guard().await.id.clone();
        let tools_guard = self.tools.read().await;
        let active_tool_names = self.active_tool_names.read().await;
        let tools = tools_guard.values().cloned().collect::<Vec<_>>();
        let active_tools =
            active_tool_names.iter().filter_map(|name| tools_guard.get(name).cloned()).collect::<Vec<_>>();
        AgentHarnessTurnState {
            messages: context.messages,
            session_id,
            model: self.model.read().await.clone(),
            thinking_level: *self.thinking_level.read().await,
            tools,
            active_tools,
        }
    }

    /// 把待写入条目顺序落盘到 Session。
    pub async fn flush_pending_session_writes(&self) -> HarnessResult<()> {
        let mut pending_session_writes = self.pending_session_writes.write().await;
        flush_session_writes(self.session.as_ref(), &mut pending_session_writes).await
    }

    /// 在已有 turn state 上执行一轮 prompt。
    pub async fn execute_turn(
        &self,
        turn_state: AgentHarnessTurnState,
        text: impl Into<String>,
        images: Option<Vec<ImageContent>>,
    ) -> AgentHarnessRuntimeResult<AssistantMessage> {
        let mut agent = self.agent.lock().await;
        let checkpoint_id =
            agent.env().create_point().await.map_err(|error| AgentHarnessError::Message(error.to_string()))?;
        let before_length = apply_turn_state(&mut agent, turn_state);
        if let Some(provider) = self.system_prompt_provider.clone() {
            let model = self.model.read().await;
            let thinking_level = self.thinking_level.read().await;
            let resources = self.resources.read().await;
            agent.state_mut().system_prompt = provider
                .system_prompt(SystemPromptContext {
                    env: agent.env(),
                    session: self.session.as_ref(),
                    model: &model,
                    thinking_level: thinking_level.as_ref(),
                    active_tools: &agent.state().tools,
                    resources: &resources,
                })
                .await;
        }

        let mut prompt = text.into();
        let mut images = images;
        let mut before_messages = Vec::new();
        {
            let resources = self.resources.read().await;
            let before_event = AgentHarnessOwnEvent::BeforeAgentStart {
                prompt: &prompt,
                images: &images,
                system_prompt: &agent.state().system_prompt,
                resources: &resources,
            };
            self.emit_own(before_event).await?;
            for hook in &self.before_agent_start_hooks {
                let mut before_event = AgentHarnessOwnEvent::BeforeAgentStart {
                    prompt: &prompt,
                    images: &images,
                    system_prompt: &agent.state().system_prompt,
                    resources: &resources,
                };
                if let Some(result) = hook.execute(&mut before_event).await? {
                    if let Some(next_prompt) = result.prompt {
                        prompt = next_prompt;
                    }
                    if let Some(mut next_messages) = result.messages {
                        before_messages.append(&mut next_messages);
                    }
                    if let Some(next_images) = result.images {
                        images = Some(next_images);
                    }
                    if let Some(next_system_prompt) = result.system_prompt {
                        agent.set_system_prompt(next_system_prompt);
                    }
                }
            }
        }
        let mut messages = vec![create_user_message(prompt, None, images)];
        if !before_messages.is_empty() {
            before_messages.extend(messages);
            messages = before_messages;
        }
        let mut queued = {
            let mut queue = self.next_turn_queue.write().await;
            std::mem::take(&mut *queue)
        };
        if !queued.is_empty() {
            *self.next_turn_count.write().await = 0;
            queued.extend(messages);
            messages = queued;
            self.emit_queue_update().await?;
        }
        self.flush_pending_session_writes().await?;
        let mut pending_session_writes = self.pending_session_writes.write().await;
        let mut run_listener = HarnessRunEventListener {
            session: Arc::clone(&self.session),
            checkpoint_id: Some(checkpoint_id.as_str()),
            pending_session_writes: &mut pending_session_writes,
            phase: &self.phase,
            next_turn_queue: &self.next_turn_queue,
            listeners: &self.listeners,
        };
        let mut run_listeners: [&mut dyn AgentEventListener; 1] = [&mut run_listener];
        let prompt_result = agent.prompt(messages, &mut run_listeners).await?;
        let completed_messages = if prompt_result.is_empty() {
            agent.state().messages.iter().cloned().skip(before_length).collect::<Vec<_>>()
        } else {
            prompt_result
        };
        completed_messages
            .into_iter()
            .rev()
            .find_map(|message| match message {
                AgentMessage::Assistant(message) => Some(message),
                _ => None,
            })
            .ok_or_else(|| {
                AgentHarnessError::Message("AgentHarness prompt completed without an assistant message".to_string())
            })
    }

    /// 向 Agent 发起一次新的对话 turn。
    pub async fn prompt(
        &self,
        text: impl Into<String>,
        images: Option<Vec<ImageContent>>,
    ) -> AgentHarnessRuntimeResult<AssistantMessage> {
        if !self.is_idle().await {
            return Err(AgentHarnessError::Message("AgentHarness is busy".to_string()));
        }
        self.set_phase(AgentHarnessPhase::Turn).await;
        let result = async {
            let turn_state = self.create_turn_state().await;
            self.execute_turn(turn_state, text, images).await
        }
        .await;
        self.set_phase(AgentHarnessPhase::Idle).await;
        result
    }

    /// 显式调用一个已加载的 Skill。
    pub async fn skill(
        &self,
        name: &str,
        additional_instructions: Option<&str>,
    ) -> AgentHarnessRuntimeResult<AssistantMessage> {
        if !self.is_idle().await {
            return Err(AgentHarnessError::Message("AgentHarness is busy".to_string()));
        }
        self.set_phase(AgentHarnessPhase::Turn).await;
        let result = async {
            let turn_state = self.create_turn_state().await;
            let prompt = {
                let resources = self.get_resources().await;
                let skill = resources
                    .skills
                    .iter()
                    .find(|candidate| candidate.name == name)
                    .ok_or_else(|| AgentHarnessError::Message(format!("Unknown skill: {name}")))?;
                let agent = self.agent.lock().await;
                format_skill_invocation(agent.env(), skill, additional_instructions)
            };
            self.execute_turn(turn_state, prompt, None).await
        }
        .await;
        self.set_phase(AgentHarnessPhase::Idle).await;
        result
    }

    /// 显式调用一个已加载的 PromptTemplate。
    pub async fn prompt_from_template(
        &self,
        name: &str,
        args: &[String],
    ) -> AgentHarnessRuntimeResult<AssistantMessage> {
        if !self.is_idle().await {
            return Err(AgentHarnessError::Message("AgentHarness is busy".to_string()));
        }
        self.set_phase(AgentHarnessPhase::Turn).await;
        let result = async {
            let turn_state = self.create_turn_state().await;
            let prompt = {
                let resources = self.get_resources().await;
                let template = resources
                    .prompt_templates
                    .iter()
                    .find(|candidate| candidate.name == name)
                    .ok_or_else(|| AgentHarnessError::Message(format!("Unknown prompt template: {name}")))?;
                format_prompt_template_invocation(template, args)
            };
            self.execute_turn(turn_state, prompt, None).await
        }
        .await;
        self.set_phase(AgentHarnessPhase::Idle).await;
        result
    }

    /// 把一条转向消息注入当前正在进行的 turn。
    pub async fn steer(
        &self,
        text: impl Into<String>,
        images: Option<Vec<ImageContent>>,
    ) -> AgentHarnessRuntimeResult<()> {
        if self.is_idle().await {
            return Err(AgentHarnessError::Message("Cannot steer while idle".to_string()));
        }
        let message = create_user_message(text.into(), None, images);
        self.handle.steer(message).await;
        self.emit_queue_update().await?;
        Ok(())
    }

    /// 把一条跟进消息注入当前正在进行的 turn。
    pub async fn follow_up(
        &self,
        text: impl Into<String>,
        images: Option<Vec<ImageContent>>,
    ) -> AgentHarnessRuntimeResult<()> {
        if self.is_idle().await {
            return Err(AgentHarnessError::Message("Cannot follow up while idle".to_string()));
        }
        let message = create_user_message(text.into(), None, images);
        self.handle.follow_up(message).await;
        self.emit_queue_update().await?;
        Ok(())
    }

    /// 把一条 user 消息加入下一轮 turn 队列。
    pub async fn next_turn(
        &self,
        text: impl Into<String>,
        images: Option<Vec<ImageContent>>,
    ) -> AgentHarnessRuntimeResult<()> {
        let next_turn_count = {
            let mut queue = self.next_turn_queue.write().await;
            queue.push(create_user_message(text.into(), None, images));
            queue.len()
        };
        *self.next_turn_count.write().await = next_turn_count;
        self.emit_queue_update().await?;
        Ok(())
    }

    /// 把一条 AgentMessage 直接追加到当前会话存储。
    pub async fn append_message(&self, message: AgentMessage) -> HarnessResult<()> {
        if self.is_idle().await {
            self.session.append_message(message, None).await?;
        } else {
            self.pending_session_writes.write().await.push(PendingSessionWrite::Message { message });
        }
        Ok(())
    }

    /// 更新当前会话名称。
    pub async fn append_session_name(&self, name: impl Into<String>) -> HarnessResult<()> {
        let name = name.into();
        if self.is_idle().await {
            self.session.append_session_name(name).await?;
        } else {
            self.pending_session_writes.write().await.push(PendingSessionWrite::SessionInfo { name: Some(name) });
        }
        Ok(())
    }

    /// 把会话当前 leaf 切换到指定聊天消息，可选生成分支总结。
    pub async fn navigate_tree(
        &self,
        index: usize,
        summarize: bool,
        custom_instructions: Option<String>,
        replace_instructions: bool,
        label: Option<String>,
    ) -> AgentHarnessRuntimeResult<NavigateTreeResult> {
        if !self.is_idle().await {
            return Err(AgentHarnessError::Message("navigateTree() requires idle harness".to_string()));
        }
        self.set_phase(AgentHarnessPhase::BranchSummary).await;
        let result = async {
            let auth = self.agent.lock().await.current_provider_auth().await;
            let old_leaf_id = self.session.get_leaf_id().await;
            let target_entry = self.session.get_chat_entry(index).await.ok_or_else(|| {
                AgentHarnessError::Message(format!("Chat message at index {index} not found"))
            })?;
            let target_id = target_entry.id();
            let (new_leaf_id, editor_text) = navigate_target_leaf(&target_entry);
            if old_leaf_id == new_leaf_id {
                return Ok(NavigateTreeResult { cancelled: false, ..Default::default() });
            }
            let collected = collect_entries_for_branch_summary(self.session.as_ref(), old_leaf_id.as_deref(), target_id).await;
            let mut summary_text = None;
            let mut summary_details = None;
            let should_summarize = summarize && !collected.entries.is_empty();
            if should_summarize {
                let model = self.model.read().await;
                let branch_summary = generate_branch_summary(collected.entries, GenerateBranchSummaryOptions { model: &model, auth: &auth, custom_instructions, replace_instructions, reserve_tokens: None }).await?;
                if branch_summary.aborted.unwrap_or(false) { return Ok(NavigateTreeResult { cancelled: true, ..Default::default() }); }
                if let Some(error) = branch_summary.error { return Err(AgentHarnessError::Message(error)); }
                summary_details = Some(serde_json::json!({ "readFiles": branch_summary.read_files.unwrap_or_default(), "modifiedFiles": branch_summary.modified_files.unwrap_or_default() }));
                summary_text = branch_summary.summary;
            }
            if let Some(checkpoint_id) = target_entry.checkpoint_id() {
                self.agent
                    .lock()
                    .await
                    .env()
                    .reset_point(checkpoint_id)
                    .await
                    .map_err(|error| AgentHarnessError::Message(error.to_string()))?;
            }
            let summary_id = self.session.move_to(new_leaf_id, summary_text.map(|summary| BranchMoveSummary { summary, details: summary_details, from_hook: Some(false) })).await?;
            if let (Some(target_id), Some(label)) = (summary_id.as_deref().or(Some(target_id)), label) {
                self.session.append_label(target_id.to_string(), Some(label)).await?;
            }
            let summary_entry = match summary_id { Some(id) => self.session.get_entry(&id).await, None => None };
            Ok(NavigateTreeResult { cancelled: false, editor_text, summary_entry })
        }.await;
        self.set_phase(AgentHarnessPhase::Idle).await;
        result
    }

    /// 返回当前生效的 Model 读锁 guard。
    pub async fn with_model(&self) -> tokio::sync::RwLockReadGuard<'_, Model> {
        self.model.read().await
    }

    /// 切换当前使用的 Model。
    pub async fn set_model(&self, model: Model) -> AgentHarnessRuntimeResult<()> {
        let (previous_model, current_model) = {
            let mut current = self.model.write().await;
            if current.provider == model.provider && current.id == model.id {
                return Ok(());
            }
            let previous_model = Some(std::mem::replace(&mut *current, model));
            (previous_model, current.clone())
        };
        if self.is_idle().await {
            self.session.append_model_change(current_model.provider.clone(), current_model.id.clone()).await?;
        } else {
            self.pending_session_writes.write().await.push(PendingSessionWrite::ModelChange {
                provider: current_model.provider.clone(),
                model_id: current_model.id.clone(),
            });
        }
        self.emit_own(AgentHarnessOwnEvent::ModelSelect {
            model: &current_model,
            previous_model: previous_model.as_ref(),
            source: ModelSelectSource::Set,
        })
        .await?;
        Ok(())
    }

    /// 切换当前 thinking level。
    pub async fn set_thinking_level(&self, level: Option<ThinkingLevel>) -> AgentHarnessRuntimeResult<()> {
        let (previous_level, current_level) = {
            let mut current = self.thinking_level.write().await;
            if *current == level {
                return Ok(());
            }
            let previous_level = std::mem::replace(&mut *current, level);
            (previous_level, *current)
        };
        if self.is_idle().await {
            self.session.append_thinking_level_change(thinking_level_to_string(current_level.as_ref())).await?;
        } else {
            self.pending_session_writes.write().await.push(PendingSessionWrite::ThinkingLevelChange {
                thinking_level: thinking_level_to_string(current_level.as_ref()),
            });
        }
        self.emit_own(AgentHarnessOwnEvent::ThinkingLevelSelect {
            level: current_level.as_ref(),
            previous_level: previous_level.as_ref(),
        })
        .await?;
        Ok(())
    }

    /// 更新当前激活工具名列表。
    pub async fn set_active_tools(&self, tool_names: Vec<String>) -> AgentHarnessRuntimeResult<()> {
        self.validate_tool_names(&tool_names).await?;
        *self.active_tool_names.write().await = tool_names;
        Ok(())
    }

    /// 返回当前资源快照。
    pub async fn get_resources(&self) -> tokio::sync::RwLockReadGuard<'_, AgentHarnessResources> {
        self.resources.read().await
    }

    /// 整体替换当前资源。
    pub async fn set_resources(&self, resources: AgentHarnessResources) -> AgentHarnessRuntimeResult<()> {
        let previous_resources = {
            let mut current = self.resources.write().await;
            std::mem::replace(&mut *current, resources)
        };
        let resources = self.resources.read().await;
        self.emit_own(AgentHarnessOwnEvent::ResourcesUpdate {
            resources: &resources,
            previous_resources: &previous_resources,
        })
        .await?;
        Ok(())
    }

    /// 在闭包内读取底层 Agent 当前 stream options。
    pub async fn with_stream_options<R>(&self, f: impl FnOnce(&StreamOptions) -> R) -> R {
        let agent = self.agent.lock().await;
        f(agent.get_stream_options())
    }

    /// 整体替换 stream options。
    pub async fn set_stream_options(
        &self,
        options: AgentHarnessStreamOptions,
        before: Vec<Box<BeforeProviderPayloadHook>>,
        after: Vec<Box<AfterProviderResponseHook>>,
    ) {
        let thinking_level = self.thinking_level.try_read().map(|level| *level).unwrap_or(None);
        self.agent.lock().await.set_stream_options(with_stream_callbacks(
            options,
            thinking_level,
            &self.listeners,
            before,
            after,
        ));
    }

    /// 重置工具注册表。
    pub async fn set_tools(
        &self,
        tools: Vec<Arc<dyn AgentTool>>,
        active_tool_names: Option<Vec<String>>,
    ) -> AgentHarnessRuntimeResult<()> {
        let tools_by_name = tools.into_iter().map(|tool| (tool.definition().name, tool)).collect();
        *self.tools.write().await = tools_by_name;
        if let Some(active_tool_names) = active_tool_names {
            self.validate_tool_names(&active_tool_names).await?;
            *self.active_tool_names.write().await = active_tool_names;
        } else {
            let active_tool_names = self.active_tool_names.read().await;
            self.validate_tool_names(&active_tool_names).await?;
        }
        Ok(())
    }

    /// 中止当前 run：清空底层队列。
    pub async fn abort(&self) -> AgentHarnessRuntimeResult<AbortResult> {
        let cleared_steer = self.handle.take_steering_queue().await;
        let cleared_follow_up = self.handle.take_follow_up_queue().await;
        self.emit_queue_update().await?;
        self.handle.abort().await;
        self.handle.wait_for_idle().await;
        let result = AbortResult { cleared_steer, cleared_follow_up };
        self.emit_own(AgentHarnessOwnEvent::Abort {
            cleared_steer: &result.cleared_steer,
            cleared_follow_up: &result.cleared_follow_up,
        })
        .await?;
        Ok(result)
    }

    /// 等待底层 Agent 进入 idle。
    pub async fn wait_for_idle(&self) {
        self.handle.wait_for_idle().await;
    }
}

/// 把 turn 状态同步到已加锁的底层 Agent。
fn apply_turn_state(agent: &mut Agent, turn_state: AgentHarnessTurnState) -> usize {
    agent.set_model(turn_state.model);
    agent.set_thinking_level(turn_state.thinking_level);
    let state = agent.state_mut();
    state.messages = turn_state.messages;
    state.tools = turn_state.active_tools;
    state.messages.len()
}

/// 把 pending session writes 队列按 TS flushPendingSessionWrites 语义顺序落盘。
async fn flush_session_writes(
    session: &Session,
    pending_session_writes: &mut Vec<PendingSessionWrite>,
) -> HarnessResult<()> {
    let writes = std::mem::take(pending_session_writes);
    for write in writes {
        match write {
            PendingSessionWrite::Message { message } => {
                session.append_message(message, None).await?;
            }
            PendingSessionWrite::ModelChange { provider, model_id } => {
                session.append_model_change(provider, model_id).await?;
            }
            PendingSessionWrite::ThinkingLevelChange { thinking_level } => {
                session.append_thinking_level_change(thinking_level).await?;
            }
            PendingSessionWrite::Custom { custom_type, data } => {
                session.append_custom_entry(custom_type, data).await?;
            }
            PendingSessionWrite::CustomMessage { custom_type, content, display, details } => {
                session.append_custom_message_entry(custom_type, content, display, details).await?;
            }
            PendingSessionWrite::Label { target_id, label } => {
                session.append_label(target_id, label).await?;
            }
            PendingSessionWrite::SessionInfo { name } => {
                session.append_session_name(name.unwrap_or_default()).await?;
            }
        }
    }
    Ok(())
}

/// 发布事件到监听器。
async fn emit_listeners(
    listeners: &[Arc<AgentHarnessListener>],
    mut event: AgentHarnessEvent<'_>,
) -> Result<(), AgentError> {
    for listener in listeners {
        listener.execute(&mut event).await?;
    }
    Ok(())
}

/// 单次 Agent run 内把底层事件桥接到 Harness。
struct HarnessRunEventListener<'a> {
    /// 当前会话。
    session: SessionHandle,
    /// 当前执行轮次开始时创建的检查点 id。
    checkpoint_id: Option<&'a str>,
    /// 待落盘的 Harness 侧会话写入。
    pending_session_writes: &'a mut Vec<PendingSessionWrite>,
    /// Harness 阶段锁。
    phase: &'a RwLock<AgentHarnessPhase>,
    /// 下一轮队列。
    next_turn_queue: &'a RwLock<Vec<AgentMessage>>,
    /// Harness 事件监听器。
    listeners: &'a [Arc<AgentHarnessListener>],
}

#[async_trait]
impl AgentEventListener for HarnessRunEventListener<'_> {
    /// 广播 AgentEvent，并同步会话落盘 / save point / settled 副作用。
    async fn execute(&mut self, event: &AgentEvent<'_>) -> Result<(), AgentError> {
        emit_listeners(self.listeners, AgentHarnessEvent::Agent(event)).await?;
        match event {
            AgentEvent::MessageEnd { message } => {
                let checkpoint_id = match message {
                    AgentMessage::User(_) => self.checkpoint_id.take(),
                    _ => None,
                };
                self.session.append_message((*message).clone(), checkpoint_id).await.map_err(listener_error)?;
            }
            AgentEvent::TurnEnd { .. } => {
                let had_pending_mutations = !self.pending_session_writes.is_empty();
                flush_session_writes(self.session.as_ref(), self.pending_session_writes)
                    .await
                    .map_err(listener_error)?;
                let event = AgentHarnessOwnEvent::SavePoint { had_pending_mutations };
                emit_listeners(self.listeners, AgentHarnessEvent::Harness(&event)).await?;
            }
            AgentEvent::AgentEnd { .. } => {
                flush_session_writes(self.session.as_ref(), self.pending_session_writes)
                    .await
                    .map_err(listener_error)?;
                *self.phase.write().await = AgentHarnessPhase::Idle;
                let next_turn_count = self.next_turn_queue.read().await.len();
                let event = AgentHarnessOwnEvent::Settled { next_turn_count };
                emit_listeners(self.listeners, AgentHarnessEvent::Harness(&event)).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// 把 Harness 侧错误转换为 Agent listener 错误。
fn listener_error(error: impl std::fmt::Display) -> AgentError {
    AgentError::Listener(error.to_string())
}

/// Harness context hook adapter。
struct HarnessContextHook {
    /// 监听器。
    listeners: Vec<Arc<AgentHarnessListener>>,
    /// hooks。
    hooks: Vec<Box<ContextHook>>,
}

#[async_trait]
impl MEHook<Vec<AgentMessage>, bool> for HarnessContextHook {
    /// 执行 context pipeline。
    async fn execute(&self, messages: &mut Vec<AgentMessage>, _env: &dyn ExecutionEnv) -> Result<bool, AgentError> {
        let event = AgentHarnessOwnEvent::Context { messages };
        emit_listeners(&self.listeners, AgentHarnessEvent::Harness(&event)).await?;
        for hook in &self.hooks {
            if let Some(result) = hook.execute(messages).await? {
                if let Some(result_messages) = result.messages {
                    *messages = result_messages;
                }
            }
        }
        Ok(true)
    }
}

/// Harness before tool hook adapter。
struct HarnessBeforeToolCallHook {
    /// 监听器。
    listeners: Vec<Arc<AgentHarnessListener>>,
    /// hooks。
    hooks: Vec<Box<ToolCallHook>>,
}

#[async_trait]
impl<'a> MHook<BeforeToolCallContext<'a>, Option<BeforeToolCallResult>> for HarnessBeforeToolCallHook {
    /// 执行 tool_call pipeline。
    async fn execute(
        &self,
        context: &mut BeforeToolCallContext<'a>,
    ) -> Result<Option<BeforeToolCallResult>, AgentError> {
        let empty_input = Default::default();
        let input = match &context.args {
            Value::Object(map) => map,
            _ => &empty_input,
        };
        let event = AgentHarnessOwnEvent::ToolCall {
            tool_call_id: &context.tool_call.id,
            tool_name: &context.tool_call.name,
            input,
        };
        emit_listeners(&self.listeners, AgentHarnessEvent::Harness(&event)).await?;
        let mut final_result = None;
        for hook in &self.hooks {
            let mut event = AgentHarnessOwnEvent::ToolCall {
                tool_call_id: &context.tool_call.id,
                tool_name: &context.tool_call.name,
                input,
            };
            if let Some(result) = hook.execute(&mut event).await? {
                final_result = Some(result);
            }
        }
        Ok(final_result)
    }
}

/// Harness after tool hook adapter。
struct HarnessAfterToolCallHook {
    /// 监听器。
    listeners: Vec<Arc<AgentHarnessListener>>,
    /// hooks。
    hooks: Vec<Box<ToolResultHook>>,
}

#[async_trait]
impl<'a> MHook<AfterToolCallContext<'a>, Option<AfterToolCallResult>> for HarnessAfterToolCallHook {
    /// 执行 tool_result pipeline。
    async fn execute(&self, context: &mut AfterToolCallContext<'a>) -> Result<Option<AfterToolCallResult>, AgentError> {
        let empty_input = Default::default();
        let input = match &context.args {
            Value::Object(map) => map,
            _ => &empty_input,
        };
        let event = AgentHarnessOwnEvent::ToolResult {
            tool_call_id: &context.tool_call.id,
            tool_name: &context.tool_call.name,
            input,
            content: &context.result.content,
            details: &context.result.details,
            is_error: *context.is_error,
        };
        emit_listeners(&self.listeners, AgentHarnessEvent::Harness(&event)).await?;
        let mut final_result = None;
        for hook in &self.hooks {
            let mut event = AgentHarnessOwnEvent::ToolResult {
                tool_call_id: &context.tool_call.id,
                tool_name: &context.tool_call.name,
                input,
                content: &context.result.content,
                details: &context.result.details,
                is_error: *context.is_error,
            };
            if let Some(result) = hook.execute(&mut event).await? {
                final_result = Some(result);
            }
        }
        Ok(final_result)
    }
}

/// Harness prepare_next_turn adapter。
struct HarnessPrepareNextTurnHook {
    /// 当前会话句柄。
    session: SessionHandle,
    /// 当前生效的 Model。
    model: Arc<RwLock<Model>>,
    /// 当前生效的 thinking level。
    thinking_level: Arc<RwLock<Option<ThinkingLevel>>>,
    /// 工具注册表。
    tools: Arc<RwLock<HashMap<String, Arc<dyn AgentTool>>>>,
    /// 当前会话激活的工具名列表。
    active_tool_names: Arc<RwLock<Vec<String>>>,
    /// Harness 当前持有的资源。
    resources: Arc<RwLock<AgentHarnessResources>>,
    /// 动态系统提示词提供者。
    system_prompt_provider: Option<Arc<dyn SystemPromptProvider>>,
}

#[async_trait]
impl<'a> MEHook<PrepareNextTurnContext<'a>, bool> for HarnessPrepareNextTurnHook {
    /// 下一轮 Provider 请求前同步 Harness 最新状态。
    async fn execute(
        &self,
        context: &mut PrepareNextTurnContext<'a>,
        env: &dyn ExecutionEnv,
    ) -> Result<bool, AgentError> {
        let session_context = self.session.build_context().await;
        let model = self.model.read().await;
        let thinking_level = self.thinking_level.read().await;
        let tools = self.tools.read().await;
        let active_tool_names = self.active_tool_names.read().await;
        let active_tools = active_tool_names.iter().filter_map(|name| tools.get(name).cloned()).collect::<Vec<_>>();

        context.context.messages = session_context.messages;
        context.context.tools = active_tools;
        *context.model = model.clone();
        context.stream_options.reasoning = *thinking_level;

        if let Some(provider) = &self.system_prompt_provider {
            let resources = self.resources.read().await;
            context.context.system_prompt = provider
                .system_prompt(SystemPromptContext {
                    env,
                    session: self.session.as_ref(),
                    model: &model,
                    thinking_level: thinking_level.as_ref(),
                    active_tools: &context.context.tools,
                    resources: &resources,
                })
                .await;
        }
        Ok(true)
    }
}

/// Harness shouldStopAfterTurn adapter。
struct HarnessShouldStopAfterTurnHook {}

#[async_trait]
impl<'a> MHook<ShouldStopAfterTurnContext<'a>, bool> for HarnessShouldStopAfterTurnHook {
    /// 默认不提前停止。
    async fn execute(&self, _context: &mut ShouldStopAfterTurnContext<'a>) -> Result<bool, AgentError> {
        Ok(false)
    }
}

/// 计算 navigateTree 的目标 leaf 与 editorText。
fn navigate_target_leaf(target_entry: &SessionTreeEntry) -> (Option<String>, Option<String>) {
    match target_entry {
        SessionTreeEntry::Message { base, message: AgentMessage::User(message) } => {
            (base.parent_id.clone(), Some(user_content_text(&message.content)))
        }
        SessionTreeEntry::CustomMessage { base, content, .. } => {
            (base.parent_id.clone(), Some(custom_content_text(content)))
        }
        _ => (Some(target_entry.id().to_string()), None),
    }
}

/// 提取 user 文本内容。
fn user_content_text(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Blocks(blocks) => {
            let mut result = String::new();
            for block in blocks {
                if let ContentBlock::Text(text) = block {
                    result.push_str(&text.text);
                }
            }
            result
        }
    }
}

/// 提取 custom message 文本内容。
fn custom_content_text(content: &CustomMessageContent) -> String {
    match content {
        CustomMessageContent::Text(text) => text.clone(),
        CustomMessageContent::Blocks(blocks) => {
            let mut result = String::new();
            for block in blocks {
                if let ContentBlock::Text(text) = block {
                    result.push_str(&text.text);
                }
            }
            result
        }
    }
}
