//! Agent 运行时使用的核心类型定义集合，包括 stream 函数协议、工具调用钩子、Agent 公开状态、
//! AgentMessage 联合类型、AgentTool 工具定义以及对外暴露的 AgentEvent 事件协议。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    sync::{atomic::AtomicBool, Arc},
};
use tokio::sync::Notify;

use crate::model::{
    api_registry::AssistantMessageEventSink,
    stream::stream_simple,
    types::{
        AssistantMessage, Auth, ContentBlock, Context, Message, Model, StreamError, StreamOptions, Tool, ToolCall,
        ToolResultMessage,
    },
};
use crate::{agent::env::ExecutionEnv, model::UserMessage};

/// Agent 错误。
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// 已有 run 在处理中。
    #[error("Agent is already processing")]
    AlreadyProcessing,
    /// 未提供 stream 函数。
    #[error("AgentOptions.stream_fn is required in Rust implementation")]
    MissingStreamFn,
    /// 没有可继续的消息。
    #[error("No messages to continue from")]
    NoMessagesToContinue,
    /// 不能从 assistant 消息继续。
    #[error("Cannot continue from message role: assistant")]
    CannotContinueFromAssistant,
    /// Agent 事件监听器处理失败。
    #[error("Agent event listener failed: {0}")]
    Listener(String),
    /// Provider stream 失败。
    #[error(transparent)]
    Stream(#[from] StreamError),
}

/// Agent loop 异步调用 Provider 的 stream 函数接口。
#[async_trait]
pub trait StreamFn: Send + Sync {
    /// 执行一次 Provider stream 请求。
    async fn stream<'a>(
        &'a self,
        model: &'a Model,
        context: Context,
        options: &'a StreamOptions,
        auth: &'a Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, AgentError>;
}

/// 默认 Provider stream 实现，直接调用 `stream_simple`。
pub struct DefaultStreamFn;

#[async_trait]
impl StreamFn for DefaultStreamFn {
    /// 执行一次默认 Provider stream 请求。
    async fn stream<'a>(
        &'a self,
        model: &'a Model,
        context: Context,
        options: &'a StreamOptions,
        auth: &'a Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, AgentError> {
        stream_simple(model, context, options, auth, sink).await.map_err(AgentError::from)
    }
}

/// 当前活动 run 的跨线程控制状态。
pub(crate) struct ActiveRunState {
    /// 当前是否有活动 run。
    pub(crate) is_active: AtomicBool,
    /// 当前活动 run 的 abort 标记。
    pub(crate) abort_flag: AtomicBool,
    /// 当前活动 run 结束时唤醒等待者。
    pub(crate) idle: Notify,
}

impl ActiveRunState {
    /// 创建空闲状态。
    pub(crate) fn new() -> Self {
        Self { is_active: AtomicBool::new(false), abort_flag: AtomicBool::new(false), idle: Notify::new() }
    }
}

/// 单条 assistant 消息中多个 tool call 的执行模式。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ToolExecutionMode {
    /// 每个 tool call 依次完成。
    Sequential,
    /// 允许并发执行的工具会并行运行。
    Parallel,
}

impl Default for ToolExecutionMode {
    fn default() -> Self {
        Self::Parallel
    }
}

/// 从一条 `AssistantMessage` 的内容数组中抽取出来的单个 tool call content block。
pub type AgentToolCall = ToolCall;

/// `before_tool_call` 钩子的返回值。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeToolCallResult {
    /// 是否阻止该 tool call 的执行。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<bool>,
    /// 阻止时回填到错误 toolResult 中的原因文案。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `after_tool_call` 钩子的返回值。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AfterToolCallResult {
    /// 覆盖 toolResult 的 content 数组。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ContentBlock>>,
    /// 覆盖 toolResult 的自定义 details 负载。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    /// 覆盖最终的错误标志。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// 提示 agent 在当前 tool 批次结束后停止后续推断。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminate: Option<bool>,
}

/// 应用层自定义 AgentMessage 类型的扩展点。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role")]
pub enum AgentMessage {
    /// LLM 标准消息。
    #[serde(rename = "user")]
    User(UserMessage),
    /// Assistant 标准消息。
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    /// ToolResult 标准消息。
    #[serde(rename = "toolResult")]
    ToolResult(ToolResultMessage),
    /// 应用层自定义消息。
    #[serde(rename = "custom")]
    Custom { kind: String, payload: Value },
}

impl AgentMessage {
    /// 尝试转移为 LLM 可消费的 `Message`。
    pub fn into_llm_message(self) -> Option<Message> {
        match self {
            AgentMessage::User(message) => Some(Message::User(message)),
            AgentMessage::Assistant(message) => Some(Message::Assistant(message)),
            AgentMessage::ToolResult(message) => Some(Message::ToolResult(message)),
            AgentMessage::Custom { .. } => None,
        }
    }
}

/// Agent 生命周期事件的异步监听器。
#[async_trait]
pub trait AgentEventListener: Send {
    /// 按订阅顺序接收并处理一个 AgentEvent。
    async fn execute(&mut self, event: &AgentEvent<'_>) -> Result<(), AgentError>;
}

/// Agent 事件下沉接收器。
#[async_trait]
pub trait AgentEventSink: Send {
    /// 异步发送一个 AgentEvent。
    async fn emit<'a>(&mut self, event: AgentEvent<'a>) -> Result<AgentEvent<'a>, AgentError>;
}

/// Agent 运行时事件下沉实现。
pub struct AgentRuntimeEventSink<'state, 'slice, 'listener> {
    /// 当前 Agent 状态。
    pub(crate) state: &'state mut AgentState,
    /// 当前事件监听器集合。
    pub(crate) listeners: &'slice mut [&'listener mut dyn AgentEventListener],
}

/// Assistant 响应事件 sink，负责同步推进最终消息与 Agent 生命周期事件。
pub(crate) struct AssistantResponseSink<'a, 'config> {
    /// 是否已添加部分消息。
    pub(crate) addedpartial: bool,
    /// Agent 事件下沉。
    pub(crate) event: &'a mut dyn AgentEventSink,
    /// 当前 loop 配置。
    pub(crate) config: &'a AgentLoopConfig<'config>,
}

impl From<Message> for AgentMessage {
    fn from(value: Message) -> Self {
        match value {
            Message::User(message) => Self::User(message),
            Message::Assistant(message) => Self::Assistant(message),
            Message::ToolResult(message) => Self::ToolResult(message),
        }
    }
}

/// Agent 的对外公开状态。
#[derive(Clone, Debug, Default)]
pub struct AgentState {
    /// 每次 Provider 请求随同发送的 system prompt。
    pub system_prompt: String,
    /// 可用的 AgentTool 列表。
    pub tools: Vec<Arc<dyn AgentTool>>,
    /// 完整对话转录。
    pub messages: Vec<AgentMessage>,
    /// 当 agent 正在处理某次 prompt 或 continuation 时为 `true`。
    pub is_streaming: bool,
    /// 当前正在流式生成的 assistant 消息。
    pub streaming_message: Option<AgentMessage>,
    /// 当前正在执行的 tool call id 集合。
    pub pending_tool_calls: HashSet<String>,
    /// 最近一次失败或被中止的 assistant 轮次产生的错误信息。
    pub error_message: Option<String>,
}

/// 工具执行的最终结果或部分增量结果。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolResult {
    /// 返回给模型的内容块。
    pub content: Vec<ContentBlock>,
    /// 调用方自定义结构化 details。
    pub details: Value,
    /// 提示 agent 在当前 tool 批次结束后停止后续推断。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminate: Option<bool>,
}

/// 工具在执行过程中向调用方推送部分结果的回调签名。
pub type UpdateToolCallHook = Arc<dyn Fn(AgentToolResult) + Send + Sync>;

/// Agent 运行时使用的 AgentTool 工具定义。
#[async_trait]
pub trait AgentTool: std::fmt::Debug + Send + Sync {
    /// 创建工具实例。
    fn new() -> Self
    where
        Self: Sized;
    /// 工具名称。
    fn name() -> &'static str
    where
        Self: Sized;
    /// 工具基础定义。
    fn definition(&self) -> Tool;
    /// 使用运行时配置构建或更新工具。
    /// @param configs 工具专属的 JSON 配置。
    fn init(&self, configs: Value) -> Result<(), AgentToolError>;
    /// 可选：在 schema 校验之前对原始 tool call 参数做兼容性预处理。
    fn prepare_arguments(&self, args: Value) -> Value {
        args
    }
    /// 执行该 tool call 的主体函数。
    async fn execute(
        &self,
        env: &dyn ExecutionEnv,
        tool_call_id: &String,
        params: &Value,
        on_update: Option<&UpdateToolCallHook>,
    ) -> Result<AgentToolResult, AgentToolError>;
    /// 单工具级别的执行模式覆盖。
    fn execution_mode(&self) -> Option<ToolExecutionMode> {
        None
    }
}

/// 工具执行错误。
#[derive(Debug, thiserror::Error)]
pub enum AgentToolError {
    /// 工具执行失败。
    #[error("{0}")]
    Message(String),
}

/// 传递给 `before_tool_call` 钩子的上下文快照。
#[derive(Debug)]
pub struct BeforeToolCallContext<'a> {
    /// 触发该 tool call 的 assistant 消息。
    pub assistant_message: &'a AssistantMessage,
    /// 原始 tool call 内容块。
    pub tool_call: &'a AgentToolCall,
    /// 经过 schema 校验后的工具参数。
    pub args: &'a Value,
    /// 当前 agent 状态。
    pub context: &'a AgentState,
}

/// 传递给 `after_tool_call` 钩子的上下文快照。
#[derive(Debug)]
pub struct AfterToolCallContext<'a> {
    /// 触发该 tool call 的 assistant 消息。
    pub assistant_message: &'a AssistantMessage,
    /// 原始 tool call 内容块。
    pub tool_call: &'a AgentToolCall,
    /// 经过 schema 校验后的工具参数。
    pub args: &'a Value,
    /// 工具实际执行得到的结果。
    pub result: &'a AgentToolResult,
    /// 当前结果是否被视为错误。
    pub is_error: &'a bool,
    /// 当前 agent 状态。
    pub context: &'a AgentState,
}

/// `should_stop_after_turn` 钩子的上下文快照。
#[derive(Debug)]
pub struct ShouldStopAfterTurnContext<'a> {
    /// 完成该轮的 assistant 消息。
    pub message: &'a AssistantMessage,
    /// 本轮 toolResult 列表。
    pub tool_results: &'a Vec<&'a ToolResultMessage>,
    /// 已追加完毕后的 agent 状态。
    pub context: &'a mut AgentState,
    /// 下一次 Provider 请求的 Model。
    pub model: &'a mut Model,
    /// 下一次 Provider 请求的 stream options。
    pub stream_options: &'a mut StreamOptions,
    /// 本次 invocation 将返回的新增消息列表。
    pub new_messages: &'a Vec<AgentMessage>,
}

/// `prepare_next_turn` 钩子的入参类型。
pub type PrepareNextTurnContext<'a> = ShouldStopAfterTurnContext<'a>;

/// 异步 hook 的只读输入通用接口。
#[async_trait]
pub trait Hook<TInput, TOutput>: Send + Sync {
    /// 执行 hook。
    async fn execute(&self, input: &TInput) -> Result<TOutput, AgentError>;
}

/// 异步 hook 的通用接口。
#[async_trait]
pub trait MHook<TInput, TOutput>: Send + Sync {
    /// 执行 hook。
    async fn execute(&self, input: &mut TInput) -> Result<TOutput, AgentError>;
}

/// 异步 hook 的只读输入通用接口，执行时可访问 Agent 运行环境。
#[async_trait]
pub trait EHook<TInput, TOutput>: Send + Sync {
    /// 执行 hook。
    async fn execute(&self, input: &TInput, env: &dyn ExecutionEnv) -> Result<TOutput, AgentError>;
}

/// 异步 hook 的通用接口，执行时可访问 Agent 运行环境。
#[async_trait]
pub trait MEHook<TInput, TOutput>: Send + Sync {
    /// 执行 hook。
    async fn execute(&self, input: &mut TInput, env: &dyn ExecutionEnv) -> Result<TOutput, AgentError>;
}

/// AgentMessage 层上下文转换钩子。
pub type TransformContextHook = dyn MEHook<Vec<AgentMessage>, bool>;

/// 工具执行前钩子。
pub type BeforeToolCallHook = dyn for<'a> MHook<BeforeToolCallContext<'a>, Option<BeforeToolCallResult>>;

/// 工具执行后钩子。
pub type AfterToolCallHook = dyn for<'a> MHook<AfterToolCallContext<'a>, Option<AfterToolCallResult>>;

/// 下一轮 Provider 请求前钩子。
pub type PrepareNextTurnHook = dyn for<'a> MEHook<PrepareNextTurnContext<'a>, bool>;

/// 每轮结束后是否停止的钩子。
pub type ShouldStopAfterTurnHook = dyn for<'a> MHook<ShouldStopAfterTurnContext<'a>, bool>;

/// AgentMessage 队列 drain 钩子。
pub type MessageQueueDrainHook<'a> = dyn MHook<(), Vec<AgentMessage>> + 'a;

/// 底层 agent loop 的运行时配置。
pub struct AgentLoopConfig<'a> {
    /// Agent 持有的执行环境。
    pub env: &'a dyn ExecutionEnv,
    /// 本轮调用使用的 Model。
    pub model: &'a mut Model,
    /// Provider stream 选项。
    pub stream_options: &'a mut StreamOptions,
    /// Provider 认证信息。
    pub provider_auth: Auth,
    /// 工具执行模式。
    pub tool_execution: &'a ToolExecutionMode,
    /// 可选上下文转换钩子。
    pub transform_context: Option<&'a TransformContextHook>,
    /// 可选工具执行前钩子。
    pub before_tool_call: Option<&'a BeforeToolCallHook>,
    /// 可选工具执行过程更新回调。
    pub update_tool_call: Option<&'a UpdateToolCallHook>,
    /// 可选工具执行后钩子。
    pub after_tool_call: Option<&'a AfterToolCallHook>,
    /// 可选下一轮准备钩子。
    pub prepare_next_turn: Option<&'a PrepareNextTurnHook>,
    /// 可选停止判定钩子。
    pub should_stop_after_turn: Option<&'a ShouldStopAfterTurnHook>,
    /// 转向消息队列 drain 回调。
    pub get_steering_messages: Option<&'a MessageQueueDrainHook<'a>>,
    /// 跟进消息队列 drain 回调。
    pub get_follow_up_messages: Option<&'a MessageQueueDrainHook<'a>>,
    /// abort 状态。
    pub abort_flag: Option<&'a AtomicBool>,
}

/// Agent 向外发布的生命周期事件。
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent<'a> {
    /// 一次 run 开始时发出。
    #[serde(rename = "agent_start")]
    AgentStart,
    /// 一次 run 的最终事件。
    #[serde(rename = "agent_end")]
    AgentEnd { messages: &'a [AgentMessage] },
    /// 一轮开始时发出。
    #[serde(rename = "turn_start")]
    TurnStart,
    /// 一轮结束时发出。
    #[serde(rename = "turn_end")]
    TurnEnd { message: &'a AgentMessage, tool_results: &'a [ToolResultMessage] },
    /// 任一消息开始时发出。
    #[serde(rename = "message_start")]
    MessageStart { message: &'a AgentMessage },
    /// assistant 消息流式生成期间发出。
    #[serde(rename = "message_update")]
    MessageUpdate { message: &'a AgentMessage },
    /// 任一消息完成时发出。
    #[serde(rename = "message_end")]
    MessageEnd { message: &'a AgentMessage },
    /// 单次工具执行开始时发出。
    #[serde(rename = "tool_execution_start")]
    ToolExecutionStart { tool_call_id: &'a str, tool_name: &'a str, args: &'a Value },
    /// 工具执行部分结果。
    #[serde(rename = "tool_execution_update")]
    ToolExecutionUpdate { tool_call_id: &'a str, tool_name: &'a str, args: &'a Value, partial_result: &'a Value },
    /// 单次工具执行完成时发出。
    #[serde(rename = "tool_execution_end")]
    ToolExecutionEnd { tool_call_id: &'a str, tool_name: &'a str, result: &'a Value, is_error: bool },
}

/// 将 Agent 状态与执行环境转换成 LLM 上下文。
pub fn to_llm_context(env: &dyn ExecutionEnv, state: &AgentState) -> Context {
    Context {
        system_prompt: Some(format!(
            "System platform: {}. Use platform-compatible commands and paths. For complex tasks, make a plan before acting.\n\n{}",
            env.platform(),
            state.system_prompt
        )),
        messages: state.messages.iter().cloned().filter_map(AgentMessage::into_llm_message).collect(),
        tools: state.tools.iter().map(|tool| tool.definition()).collect(),
    }
}

/// 从 tool call 参数 map 构造 JSON Value。
pub fn tool_args_value(args: &Map<String, Value>) -> Value {
    Value::Object(args.clone())
}
