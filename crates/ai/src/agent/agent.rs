//! 底层 agent loop 之上的有状态包装，统一持有对话转录、生命周期事件、工具执行调度
//! 以及转向消息（Steering）/ 跟进消息（Follow-up）队列的管理。

use std::{
    collections::{HashSet, VecDeque},
    panic::AssertUnwindSafe,
    sync::{atomic::Ordering, Arc},
};

use async_trait::async_trait;
use futures::FutureExt;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::{
    agent::{
        agent_loop::{agent_loop, AgentLoopError},
        env::{default_env, ExecutionEnv},
        types::*,
    },
    model::types::{
        empty_usage, AssistantMessage, Auth, AuthProvider, ContentBlock, ImageContent, Message, Model, StopReason,
        StreamOptions, TextContent, ThinkingLevel, UserContent, UserMessage,
    },
};

/// 队列消耗模式。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum QueueMode {
    /// 每次轮询一次性取出当前队列中的全部消息。
    All,
    /// 每次轮询只取出最早的一条消息。
    OneAtATime,
}

impl Default for QueueMode {
    fn default() -> Self {
        Self::OneAtATime
    }
}

/// 构造 `Agent` 时使用的选项集合。
#[derive(Default)]
pub struct AgentOptions {
    /// Agent 使用工具时的运行环境（沙盒）。
    pub env: Option<Arc<dyn ExecutionEnv>>,
    /// 初始 Model。
    pub model: Model,
    /// 初始 AgentState 的快照。
    pub initial_state: Option<AgentState>,
    /// 自定义 stream 函数。
    pub stream_fn: Option<Box<dyn StreamFn>>,
    /// 转向消息队列模式。
    pub steering_mode: QueueMode,
    /// 跟进消息队列模式。
    pub follow_up_mode: QueueMode,
    /// 流式选项。
    pub stream_options: StreamOptions,
    /// Provider 认证提供者。
    pub auth_provider: Option<Arc<dyn AuthProvider>>,
    /// 工具执行模式。
    pub tool_execution: ToolExecutionMode,
    /// 可选上下文转换钩子。
    pub transform_context: Option<Box<TransformContextHook>>,
    /// 可选工具执行前钩子。
    pub before_tool_call: Option<Box<BeforeToolCallHook>>,
    /// 可选工具执行过程更新回调。
    pub update_tool_call: Option<UpdateToolCallHook>,
    /// 可选工具执行后钩子。
    pub after_tool_call: Option<Box<AfterToolCallHook>>,
    /// 可选下一轮准备钩子。
    pub prepare_next_turn: Option<Box<PrepareNextTurnHook>>,
    /// 可选停止判定钩子。
    pub should_stop_after_turn: Option<Box<ShouldStopAfterTurnHook>>,
}

/// 内部使用的待处理 AgentMessage 队列。
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PendingMessageQueue {
    /// 当前队列模式。
    pub mode: QueueMode,
    /// FIFO 存储。
    messages: VecDeque<AgentMessage>,
}

impl PendingMessageQueue {
    /// 创建一个空队列。
    pub fn new(mode: QueueMode) -> Self {
        Self { mode, messages: VecDeque::new() }
    }

    /// 入队一条 AgentMessage。
    pub fn enqueue(&mut self, message: AgentMessage) {
        self.messages.push_back(message);
    }

    /// 队列是否还有待处理消息。
    pub fn has_items(&self) -> bool {
        !self.messages.is_empty()
    }

    /// 返回队列中的消息数量。
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// 返回队列中消息的只读迭代器。
    pub fn iter(&self) -> impl Iterator<Item = &AgentMessage> {
        self.messages.iter()
    }

    /// 按当前 mode 排空或部分排空队列。
    pub fn drain(&mut self) -> Vec<AgentMessage> {
        match self.mode {
            QueueMode::All => self.messages.drain(..).collect(),
            QueueMode::OneAtATime => self.messages.pop_front().into_iter().collect(),
        }
    }

    /// 立即清空队列中的所有待处理消息。
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// 清空队列并返回被清空的消息。
    pub fn take_all(&mut self) -> Vec<AgentMessage> {
        self.messages.drain(..).collect()
    }
}

/// 可跨任务共享的 Agent 运行控制句柄。
pub struct AgentHandle {
    /// 转向消息队列。
    steering_queue: RwLock<PendingMessageQueue>,
    /// 跟进消息队列。
    follow_up_queue: RwLock<PendingMessageQueue>,
    /// 当前活动 run 的跨线程控制状态。
    active_run: ActiveRunState,
}

impl AgentHandle {
    /// 创建一个 Agent 运行控制句柄。
    ///
    /// # 参数
    /// - `steering_mode`: 转向消息队列模式。
    /// - `follow_up_mode`: 跟进消息队列模式。
    pub fn new(steering_mode: QueueMode, follow_up_mode: QueueMode) -> Self {
        Self {
            steering_queue: RwLock::new(PendingMessageQueue::new(steering_mode)),
            follow_up_queue: RwLock::new(PendingMessageQueue::new(follow_up_mode)),
            active_run: ActiveRunState::new(),
        }
    }

    /// 入队一条转向消息。
    ///
    /// # 参数
    /// - `message`: 待注入的 AgentMessage。
    pub async fn steer(&self, message: AgentMessage) {
        self.steering_queue.write().await.enqueue(message);
    }

    /// 入队一条跟进消息。
    ///
    /// # 参数
    /// - `message`: 待注入的 AgentMessage。
    pub async fn follow_up(&self, message: AgentMessage) {
        self.follow_up_queue.write().await.enqueue(message);
    }

    /// 清空转向队列。
    pub async fn clear_steering_queue(&self) {
        self.steering_queue.write().await.clear();
    }

    /// 返回转向队列只读引用。
    pub async fn get_steering_queue(&self) -> tokio::sync::RwLockReadGuard<'_, PendingMessageQueue> {
        self.steering_queue.read().await
    }

    /// 清空跟进队列。
    pub async fn clear_follow_up_queue(&self) {
        self.follow_up_queue.write().await.clear();
    }

    /// 返回跟进队列只读引用。
    pub async fn get_follow_up_queue(&self) -> tokio::sync::RwLockReadGuard<'_, PendingMessageQueue> {
        self.follow_up_queue.read().await
    }

    /// 清空转向队列并返回被清空的消息。
    pub async fn take_steering_queue(&self) -> Vec<AgentMessage> {
        self.steering_queue.write().await.take_all()
    }

    /// 清空跟进队列并返回被清空的消息。
    pub async fn take_follow_up_queue(&self) -> Vec<AgentMessage> {
        self.follow_up_queue.write().await.take_all()
    }

    /// 同时清空两个队列。
    pub async fn clear_all_queues(&self) {
        self.clear_steering_queue().await;
        self.clear_follow_up_queue().await;
    }

    /// 当任一队列仍有未处理消息时返回 true。
    pub async fn has_queued_messages(&self) -> bool {
        self.steering_queue.read().await.has_items() || self.follow_up_queue.read().await.has_items()
    }

    /// 判断当前 Agent 是否空闲。
    pub fn is_idle(&self) -> bool {
        !self.active_run.is_active.load(Ordering::SeqCst)
    }

    /// 当前 run 的 abort 标记状态；无活动 run 时返回 None。
    pub async fn abort_flag(&self) -> Option<bool> {
        if self.active_run.is_active.load(Ordering::SeqCst) {
            Some(self.active_run.abort_flag.load(Ordering::SeqCst))
        } else {
            None
        }
    }

    /// 中止当前正在进行的 run；无活动 run 时无副作用。
    pub async fn abort(&self) {
        if self.active_run.is_active.load(Ordering::SeqCst) {
            self.active_run.abort_flag.store(true, Ordering::SeqCst);
        }
    }

    /// 等待当前 run 完成并进入 idle。
    pub async fn wait_for_idle(&self) {
        loop {
            let notified = self.active_run.idle.notified();
            if !self.active_run.is_active.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }
}

/// 底层 agent loop 之上的有状态包装。
pub struct Agent {
    /// Agent 使用工具时的运行环境（沙盒）。
    env: Arc<dyn ExecutionEnv>,
    /// 后续轮次将使用的活动 Model。
    model: Model,
    /// 内部可变状态实例。
    state: AgentState,
    /// 可跨任务共享的运行控制句柄。
    handle: Arc<AgentHandle>,
    /// 调用 Provider 的 stream 函数。
    stream_fn: Box<dyn StreamFn>,
    /// 流式选项。
    stream_options: StreamOptions,
    /// Provider 认证提供者。
    auth_provider: Option<Arc<dyn AuthProvider>>,
    /// 工具执行模式。
    tool_execution: ToolExecutionMode,
    /// 可选上下文转换钩子。
    transform_context: Option<Box<TransformContextHook>>,
    /// 可选工具执行前钩子。
    before_tool_call: Option<Box<BeforeToolCallHook>>,
    /// 可选工具执行过程更新回调。
    update_tool_call: Option<UpdateToolCallHook>,
    /// 可选工具执行后钩子。
    after_tool_call: Option<Box<AfterToolCallHook>>,
    /// 可选下一轮准备钩子。
    prepare_next_turn: Option<Box<PrepareNextTurnHook>>,
    /// 可选停止判定钩子。
    should_stop_after_turn: Option<Box<ShouldStopAfterTurnHook>>,
}

impl Agent {
    /// 根据 `AgentOptions` 构造一个 Agent 实例。
    pub fn new(options: AgentOptions) -> Result<Self, AgentError> {
        let stream_fn = options.stream_fn.unwrap_or_else(|| Box::new(DefaultStreamFn));
        Ok(Self {
            env: options.env.unwrap_or_else(default_env),
            model: options.model,
            state: options.initial_state.unwrap_or_default(),
            handle: Arc::new(AgentHandle::new(options.steering_mode, options.follow_up_mode)),
            stream_fn,
            stream_options: options.stream_options,
            auth_provider: options.auth_provider,
            tool_execution: options.tool_execution,
            transform_context: options.transform_context,
            before_tool_call: options.before_tool_call,
            update_tool_call: options.update_tool_call,
            after_tool_call: options.after_tool_call,
            prepare_next_turn: options.prepare_next_turn,
            should_stop_after_turn: options.should_stop_after_turn,
        })
    }

    /// 返回可跨任务共享的运行控制句柄。
    pub fn handle(&self) -> Arc<AgentHandle> {
        Arc::clone(&self.handle)
    }

    /// 返回 Agent 持有的执行环境。
    pub fn env(&self) -> &dyn ExecutionEnv {
        self.env.as_ref()
    }

    /// 克隆执行环境的共享句柄，供 Agent 内部回调长期持有。
    pub fn env_handle(&self) -> Arc<dyn ExecutionEnv> {
        Arc::clone(&self.env)
    }

    /// 当前 Agent 状态引用。
    pub fn state(&self) -> &AgentState {
        &self.state
    }

    /// 当前 Agent 状态可变引用。
    pub fn state_mut(&mut self) -> &mut AgentState {
        &mut self.state
    }

    /// 当前 Agent 状态快照。
    pub fn state_snapshot(&self) -> AgentState {
        self.state.clone()
    }

    /// 用新状态替换当前 Agent 状态快照。
    pub fn replace_state(&mut self, next_state: AgentState) {
        self.state = next_state;
    }

    /// 更新后续轮次使用的 system prompt。
    pub fn set_system_prompt(&mut self, system_prompt: impl Into<String>) {
        self.state.system_prompt = system_prompt.into();
    }

    /// 更新后续轮次使用的 Model。
    pub fn set_model(&mut self, model: Model) {
        self.model = model;
    }

    /// 更新后续轮次使用的 thinking level。
    pub fn set_thinking_level(&mut self, thinking_level: Option<ThinkingLevel>) {
        self.stream_options.reasoning = thinking_level;
    }

    /// 更新后续轮次暴露给 Agent 的工具列表。
    pub fn set_tools(&mut self, tools: Vec<Arc<dyn AgentTool>>) {
        self.state.tools = tools;
    }

    /// 更新 Provider stream options 快照。
    pub fn set_stream_options(&mut self, stream_options: StreamOptions) {
        self.stream_options = stream_options;
    }

    /// 返回 Provider stream options 快照引用。
    pub fn get_stream_options(&self) -> &StreamOptions {
        &self.stream_options
    }

    /// 更新 Provider 认证提供者。
    pub fn set_auth_provider(&mut self, auth_provider: Option<Arc<dyn AuthProvider>>) {
        self.auth_provider = auth_provider;
    }

    /// 按指定 Model 解析 Provider 认证信息，缺省时返回空认证。
    pub async fn provider_auth_for_model(&self, model: &Model) -> Auth {
        match &self.auth_provider {
            Some(auth_provider) => auth_provider.api_key_and_headers(model).await.unwrap_or_default(),
            None => Auth::default(),
        }
    }

    /// 按当前 Model 解析 Provider 认证信息，缺省时返回空认证。
    pub async fn current_provider_auth(&self) -> Auth {
        self.provider_auth_for_model(&self.model).await
    }

    /// 更新上下文转换钩子。
    pub fn set_transform_context(&mut self, hook: Option<Box<TransformContextHook>>) {
        self.transform_context = hook;
    }

    /// 更新工具执行前钩子。
    pub fn set_before_tool_call(&mut self, hook: Option<Box<BeforeToolCallHook>>) {
        self.before_tool_call = hook;
    }

    /// 更新工具执行过程更新回调。
    pub fn set_update_tool_call(&mut self, callback: Option<UpdateToolCallHook>) {
        self.update_tool_call = callback;
    }

    /// 更新工具执行后钩子。
    pub fn set_after_tool_call(&mut self, hook: Option<Box<AfterToolCallHook>>) {
        self.after_tool_call = hook;
    }

    /// 更新下一轮准备钩子。
    pub fn set_prepare_next_turn(&mut self, hook: Option<Box<PrepareNextTurnHook>>) {
        self.prepare_next_turn = hook;
    }

    /// 更新停止判定钩子。
    pub fn set_should_stop_after_turn(&mut self, hook: Option<Box<ShouldStopAfterTurnHook>>) {
        self.should_stop_after_turn = hook;
    }

    /// 清空对话转录、运行时状态与全部待处理队列。
    pub async fn reset(&mut self) {
        self.state.messages.clear();
        self.state.is_streaming = false;
        self.state.streaming_message = None;
        self.state.pending_tool_calls = HashSet::new();
        self.state.error_message = None;
        self.handle.clear_all_queues().await;
    }

    /// 启动一次新的文本 prompt。
    pub async fn prompt_text(
        &mut self,
        input: impl Into<String>,
        images: Vec<ImageContent>,
        listeners: &mut [&mut dyn AgentEventListener],
    ) -> Result<Vec<AgentMessage>, AgentError> {
        let mut content = vec![ContentBlock::Text(TextContent { text: input.into(), text_signature: None })];
        content.extend(images.into_iter().map(ContentBlock::Image));
        let message = AgentMessage::User(UserMessage {
            content: UserContent::Blocks(content),
            timestamp: crate::model::types::now_millis(),
        });
        self.prompt(vec![message], listeners).await
    }

    /// 启动一次新的 prompt。
    pub async fn prompt(
        &mut self,
        messages: Vec<AgentMessage>,
        listeners: &mut [&mut dyn AgentEventListener],
    ) -> Result<Vec<AgentMessage>, AgentError> {
        self.run_prompt_messages(messages, listeners).await
    }

    /// 从当前对话转录继续推进。
    pub async fn continue_run(
        &mut self,
        listeners: &mut [&mut dyn AgentEventListener],
    ) -> Result<Vec<AgentMessage>, AgentError> {
        let is_last_assistant =
            matches!(self.state.messages.last().ok_or(AgentError::NoMessagesToContinue)?, AgentMessage::Assistant(_));
        if is_last_assistant {
            let steering = self.handle.steering_queue.write().await.drain();
            if !steering.is_empty() {
                return self.run_prompt_messages(steering, listeners).await;
            }
            let follow_up = self.handle.follow_up_queue.write().await.drain();
            if !follow_up.is_empty() {
                return self.run_prompt_messages(follow_up, listeners).await;
            }
            return Err(AgentError::CannotContinueFromAssistant);
        }
        self.run_prompt_messages(Vec::new(), listeners).await
    }

    /// 执行底层 loop 并桥接事件。
    async fn run_prompt_messages(
        &mut self,
        messages: Vec<AgentMessage>,
        listeners: &mut [&mut dyn AgentEventListener],
    ) -> Result<Vec<AgentMessage>, AgentError> {
        if self.handle.active_run.is_active.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(AgentError::AlreadyProcessing);
        }
        self.handle.active_run.abort_flag.store(false, Ordering::SeqCst);
        self.state.is_streaming = true;
        self.state.streaming_message = None;
        self.state.error_message = None;

        let provider_auth = self.current_provider_auth().await;
        let stream_fn = self.stream_fn.as_ref();
        let steering_hook = QueueDrainHook { queue: &self.handle.steering_queue };
        let follow_up_hook = QueueDrainHook { queue: &self.handle.follow_up_queue };
        let config = AgentLoopConfig {
            env: self.env.as_ref(),
            model: &mut self.model,
            stream_options: &mut self.stream_options,
            provider_auth,
            tool_execution: &self.tool_execution,
            transform_context: self.transform_context.as_deref(),
            before_tool_call: self.before_tool_call.as_deref(),
            update_tool_call: self.update_tool_call.as_ref(),
            after_tool_call: self.after_tool_call.as_deref(),
            prepare_next_turn: self.prepare_next_turn.as_deref(),
            should_stop_after_turn: self.should_stop_after_turn.as_deref(),
            get_steering_messages: Some(&steering_hook),
            get_follow_up_messages: Some(&follow_up_hook),
            abort_flag: Some(&self.handle.active_run.abort_flag),
        };
        let loop_result = {
            let mut emit = AgentRuntimeEventSink { state: &mut self.state, listeners };
            AssertUnwindSafe(agent_loop(messages, config, &mut emit, stream_fn)).catch_unwind().await
        };
        let new_messages = match loop_result {
            Ok(Ok(messages)) => messages,
            Ok(Err(AgentLoopError::Event(error))) => {
                self.finish_run().await;
                return Err(error);
            }
            Ok(Err(error)) => {
                let aborted = self.handle.abort_flag().await.unwrap_or(false);
                let message = failure_message(
                    &self.model,
                    if aborted { StopReason::Aborted } else { StopReason::Error },
                    error.to_string(),
                );
                if let Err(error) = emit_failure_sequence(&mut self.state, listeners, &message).await {
                    self.finish_run().await;
                    return Err(error);
                }
                vec![message]
            }
            Err(error) => {
                let aborted = self.handle.abort_flag().await.unwrap_or(false);
                let message = failure_message(
                    &self.model,
                    if aborted { StopReason::Aborted } else { StopReason::Error },
                    panic_message(error),
                );
                if let Err(error) = emit_failure_sequence(&mut self.state, listeners, &message).await {
                    self.finish_run().await;
                    return Err(error);
                }
                vec![message]
            }
        };
        self.finish_run().await;
        Ok(new_messages)
    }

    /// 标记当前 run 结束。
    async fn finish_run(&mut self) {
        self.state.is_streaming = false;
        self.state.streaming_message = None;
        self.state.pending_tool_calls.clear();
        self.handle.active_run.abort_flag.store(false, Ordering::SeqCst);
        self.handle.active_run.is_active.store(false, Ordering::SeqCst);
        self.handle.active_run.idle.notify_waiters();
    }
}

/// 同步 drain `PendingMessageQueue` 的 adapter。
struct QueueDrainHook<'a> {
    /// 目标队列。
    queue: &'a RwLock<PendingMessageQueue>,
}

#[async_trait]
impl MHook<(), Vec<AgentMessage>> for QueueDrainHook<'_> {
    /// Drain 当前队列。
    async fn execute(&self, _input: &mut ()) -> Result<Vec<AgentMessage>, AgentError> {
        Ok(self.queue.write().await.drain())
    }
}

#[async_trait]
impl AgentEventSink for AgentRuntimeEventSink<'_, '_, '_> {
    /// 异步处理一个 AgentEvent。
    async fn emit<'a>(&mut self, event: AgentEvent<'a>) -> Result<AgentEvent<'a>, AgentError> {
        process_event(self.state, self.listeners, event).await
    }
}

/// 接收底层 loop 事件并推进内部状态。
async fn process_event<'a>(
    state: &mut AgentState,
    listeners: &mut [&mut dyn AgentEventListener],
    event: AgentEvent<'a>,
) -> Result<AgentEvent<'a>, AgentError> {
    match &event {
        AgentEvent::MessageStart { message } | AgentEvent::MessageUpdate { message, .. } => {
            state.streaming_message = Some((*message).clone());
        }
        AgentEvent::MessageEnd { message } => {
            state.streaming_message = None;
            state.messages.push((*message).clone());
        }
        AgentEvent::ToolExecutionStart { tool_call_id, .. } => {
            state.pending_tool_calls.insert((*tool_call_id).to_string());
        }
        AgentEvent::ToolExecutionEnd { tool_call_id, .. } => {
            state.pending_tool_calls.remove(*tool_call_id);
        }
        AgentEvent::TurnEnd { message, .. } => {
            if let AgentMessage::Assistant(assistant) = message {
                if assistant.error_message.is_some() {
                    state.error_message = assistant.error_message.clone();
                }
            }
        }
        AgentEvent::AgentEnd { .. } => {
            state.streaming_message = None;
        }
        _ => {}
    }

    for listener in listeners.iter_mut() {
        listener.execute(&event).await?;
    }
    Ok(event)
}

/// 默认的 `convert_to_llm` 实现。
pub fn default_convert_to_llm(messages: Vec<AgentMessage>) -> Vec<Message> {
    messages.into_iter().filter_map(AgentMessage::into_llm_message).collect()
}

/// 构造兜底失败消息。
pub fn failure_message(model: &Model, reason: StopReason, error_message: String) -> AgentMessage {
    AgentMessage::Assistant(AssistantMessage {
        content: vec![ContentBlock::Text(TextContent { text: String::new(), text_signature: None })],
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        diagnostics: vec![],
        usage: empty_usage(),
        stop_reason: reason,
        error_message: Some(error_message),
        timestamp: crate::model::types::now_millis(),
    })
}

/// 按 TS `handleRunFailure` 顺序发送兜底失败事件。
async fn emit_failure_sequence(
    state: &mut AgentState,
    listeners: &mut [&mut dyn AgentEventListener],
    message: &AgentMessage,
) -> Result<(), AgentError> {
    process_event(state, listeners, AgentEvent::MessageStart { message }).await?;
    process_event(state, listeners, AgentEvent::MessageEnd { message }).await?;
    process_event(state, listeners, AgentEvent::TurnEnd { message, tool_results: &[] }).await?;
    process_event(state, listeners, AgentEvent::AgentEnd { messages: std::slice::from_ref(message) }).await?;
    Ok(())
}

/// 从 panic payload 中提取可读错误信息。
fn panic_message(error: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = error.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = error.downcast_ref::<&'static str>() {
        return (*message).to_string();
    }
    "Agent loop panicked".to_string()
}
