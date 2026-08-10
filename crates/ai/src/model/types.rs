//! 这里只抽取 Agent 依赖到的核心框架与类型；具体 Provider 位于 `model::providers`。

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::HashMap, sync::Arc};
use time::OffsetDateTime;

use crate::model::utils::diagnostics::AssistantMessageDiagnostic;

/// 生成 Unix 毫秒时间戳。
pub fn now_millis() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000
}

/// 内置文本/聊天 API 标识枚举的开放型别名。
pub type Api = String;
/// 文本类 Provider 标识的开放型别名。
pub type Provider = String;

/// 跨 Provider 通用的“思考档位”枚举（不含 off）。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingLevel {
    /// minimal 档位。
    #[serde(rename = "minimal")]
    Minimal,
    /// low 档位。
    #[serde(rename = "low")]
    Low,
    /// medium 档位。
    #[serde(rename = "medium")]
    Medium,
    /// high 档位。
    #[serde(rename = "high")]
    High,
    /// xhigh 档位。
    #[serde(rename = "xhigh")]
    XHigh,
}

/// 模型对各思考档位的映射表。
pub type ThinkingLevelMap = HashMap<ThinkingLevel, Option<String>>;

impl std::fmt::Display for ThinkingLevel {
    /// 输出档位名。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            ThinkingLevel::Minimal => "minimal",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
        })
    }
}

/// 把可选 thinking level 转成稳定字符串；空值表示 off。
pub fn thinking_level_to_string(level: Option<&ThinkingLevel>) -> String {
    match level {
        Some(ThinkingLevel::Minimal) => "minimal".to_string(),
        Some(ThinkingLevel::Low) => "low".to_string(),
        Some(ThinkingLevel::Medium) => "medium".to_string(),
        Some(ThinkingLevel::High) => "high".to_string(),
        Some(ThinkingLevel::XHigh) => "xhigh".to_string(),
        None => "off".to_string(),
    }
}

/// 各思考档位对应的 token 预算。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingBudgets {
    /// minimal 档位的最大思考 token 数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimal: Option<u32>,
    /// low 档位的最大思考 token 数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low: Option<u32>,
    /// medium 档位的最大思考 token 数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medium: Option<u32>,
    /// high 档位的最大思考 token 数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high: Option<u32>,
}

/// 提示缓存保留时长偏好。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CacheRetention {
    /// 不使用缓存。
    #[serde(rename = "none")]
    None,
    /// 短缓存。
    #[serde(rename = "short")]
    Short,
    /// 长缓存。
    #[serde(rename = "long")]
    Long,
}

/// 跨多种传输协议的传输偏好枚举。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
    /// SSE。
    Sse,
    /// WebSocket。
    Websocket,
    /// 缓存版 WebSocket。
    WebsocketCached,
    /// 自动选择。
    Auto,
}

/// 通用 Provider HTTP 响应描述。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderResponse {
    /// HTTP 状态码。
    pub status: u16,
    /// HTTP 响应头键值对。
    pub headers: HashMap<String, String>,
}

/// stream 路由错误。
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// 对应 API 未注册。
    #[error("No API provider registered for api: {0}")]
    NoApiProvider(String),
    /// 具体 Provider 流处理失败。
    #[error("Provider stream failed: {0}")]
    Stream(String),
    /// Provider 回调执行失败。
    #[error("Provider callback failed: {0}")]
    Callback(String),
}

/// Provider payload 发出前的异步回调。
#[async_trait]
pub trait ProviderPayloadCallback: Send + Sync {
    /// 接收 Provider payload 所有权，并返回最终 Provider payload。
    async fn on_payload(&self, payload: Value, model: &Model) -> Result<Value, StreamError>;
}

/// Provider HTTP 响应到达后的异步回调。
#[async_trait]
pub trait ProviderResponseCallback: Send + Sync {
    /// 接收 Provider HTTP 响应所有权，并返回最终 Provider HTTP 响应。
    async fn on_response(&self, response: ProviderResponse, model: &Model) -> Result<ProviderResponse, StreamError>;
}

/// Provider 认证信息。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Auth {
    /// API Key。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// 认证相关 headers。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}

/// Provider 认证回调。
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// 按 model 返回本次请求认证信息。
    async fn api_key_and_headers<'a>(&'a self, model: &'a Model) -> Option<Auth>;
}

/// 文本/聊天 Provider 共享的调用选项。
#[derive(Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamOptions {
    /// 原 `SimpleStreamOptions` 使用的统一思考档位。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ThinkingLevel>,
    /// 原 `SimpleStreamOptions` 使用的各档位思考 token 预算。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budgets: Option<ThinkingBudgets>,
    /// 采样温度。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// 单次回复允许的最大输出 token 数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// 传输偏好。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    /// Prompt cache 保留偏好。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_retention: Option<CacheRetention>,
    /// 会话标识。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Provider payload 发出前的异步回调。
    #[serde(skip)]
    #[schemars(skip)]
    pub on_payload: Option<Arc<dyn ProviderPayloadCallback>>,
    /// Provider HTTP 响应到达后的异步回调。
    #[serde(skip)]
    #[schemars(skip)]
    pub on_response: Option<Arc<dyn ProviderResponseCallback>>,
    /// 附加的自定义 HTTP 请求头。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// HTTP 请求超时（毫秒）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// 客户端重试上限。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    /// 服务端要求等待重试时，本地允许的最长延迟（毫秒）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retry_delay_ms: Option<u64>,
    /// 附加在请求中的元信息字典。
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

impl std::fmt::Debug for StreamOptions {
    /// 调试输出时隐藏不可序列化回调。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StreamOptions")
            .field("reasoning", &self.reasoning)
            .field("thinking_budgets", &self.thinking_budgets)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("transport", &self.transport)
            .field("cache_retention", &self.cache_retention)
            .field("session_id", &self.session_id)
            .field("headers", &self.headers)
            .field("timeout_ms", &self.timeout_ms)
            .field("max_retries", &self.max_retries)
            .field("max_retry_delay_ms", &self.max_retry_delay_ms)
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl PartialEq for StreamOptions {
    /// 比较可序列化字段；回调与 TS 一样属于运行时能力，不参与值相等判断。
    fn eq(&self, other: &Self) -> bool {
        self.reasoning == other.reasoning
            && self.thinking_budgets == other.thinking_budgets
            && self.temperature == other.temperature
            && self.max_tokens == other.max_tokens
            && self.transport == other.transport
            && self.cache_retention == other.cache_retention
            && self.session_id == other.session_id
            && self.headers == other.headers
            && self.timeout_ms == other.timeout_ms
            && self.max_retries == other.max_retries
            && self.max_retry_delay_ms == other.max_retry_delay_ms
            && self.metadata == other.metadata
    }
}

/// 文本块的签名结构 V1。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextSignatureV1 {
    /// 协议版本号，固定为 1。
    pub v: u8,
    /// 文本块在上游 API 内的唯一标识。
    pub id: String,
    /// 可选阶段标记。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// 文本内容块。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextContent {
    /// 文本正文。
    pub text: String,
    /// OpenAI Responses 等 API 的元信息。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_signature: Option<String>,
}

/// 思考 / 推理内容块。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingContent {
    /// 思考正文。
    pub thinking: String,
    /// reasoning item id 或加密 payload。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_signature: Option<String>,
    /// 是否被安全过滤器加密屏蔽。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redacted: Option<bool>,
}

/// 图像内容块。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    /// Base64 编码的图像数据。
    pub data: String,
    /// MIME 类型。
    pub mime_type: String,
}

/// 工具调用内容块。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// 工具调用的全局唯一 id。
    pub id: String,
    /// 被调用工具的名称。
    pub name: String,
    /// 工具参数对象。
    #[serde(default)]
    pub arguments: Map<String, Value>,
    /// Google 专属不透明签名。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

/// 消息内容块。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// 文本块。
    #[serde(rename = "text")]
    Text(TextContent),
    /// 思考块。
    #[serde(rename = "thinking")]
    Thinking(ThinkingContent),
    /// 图像块。
    #[serde(rename = "image")]
    Image(ImageContent),
    /// 工具调用块。
    #[serde(rename = "toolCall")]
    ToolCall(ToolCall),
}

/// 单次调用的 Token 用量与计费明细。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// 输入 token 数。
    pub input: u64,
    /// 输出 token 数。
    pub output: u64,
    /// 命中 prompt 缓存的 token 数。
    pub cache_read: u64,
    /// 写入 prompt 缓存的 token 数。
    pub cache_write: u64,
    /// 全部 token 数。
    pub total_tokens: u64,
    /// 计费明细子对象。
    pub cost: UsageCost,
}

/// 计费明细。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageCost {
    /// 输入部分费用。
    pub input: f64,
    /// 输出部分费用。
    pub output: f64,
    /// 缓存命中费用。
    pub cache_read: f64,
    /// 缓存写入费用。
    pub cache_write: f64,
    /// 总费用。
    pub total: f64,
}

/// 单次回复的终止原因。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// 正常完成。
    #[serde(rename = "stop")]
    Stop,
    /// 达到上限。
    #[serde(rename = "length")]
    Length,
    /// 工具调用。
    #[serde(rename = "toolUse")]
    ToolUse,
    /// 发生错误。
    #[serde(rename = "error")]
    Error,
    /// 被取消。
    #[serde(rename = "aborted")]
    Aborted,
}

impl Default for StopReason {
    /// 默认按正常完成处理。
    fn default() -> Self {
        Self::Stop
    }
}

/// 用户消息内容。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum UserContent {
    /// 纯字符串。
    Text(String),
    /// 文本/图像块数组。
    Blocks(Vec<ContentBlock>),
}

/// 用户消息。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    /// 消息正文。
    pub content: UserContent,
    /// Unix 毫秒时间戳。
    pub timestamp: i64,
}

/// 助手消息。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    /// 内容块数组。
    pub content: Vec<ContentBlock>,
    /// 实际使用的 API 标识。
    pub api: Api,
    /// 实际使用的 Provider 标识。
    pub provider: Provider,
    /// 调用方请求的模型 id。
    pub model: String,
    /// Provider 实际选用的模型 id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_model: Option<String>,
    /// 上游响应/消息 id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    /// 脱敏诊断信息。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<AssistantMessageDiagnostic>,
    /// token 用量。
    pub usage: Usage,
    /// 终止原因。
    pub stop_reason: StopReason,
    /// 错误描述。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Unix 毫秒时间戳。
    pub timestamp: i64,
}

impl Default for AssistantMessage {
    /// 构造临时空 assistant 消息，主要用于所有权转移占位。
    fn default() -> Self {
        Self {
            content: Vec::new(),
            api: String::new(),
            provider: String::new(),
            model: String::new(),
            response_model: None,
            response_id: None,
            diagnostics: Vec::new(),
            usage: Usage::default(),
            stop_reason: StopReason::default(),
            error_message: None,
            timestamp: now_millis(),
        }
    }
}

/// 工具结果消息。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultMessage {
    /// 与之对应的 `ToolCall.id`。
    pub tool_call_id: String,
    /// 工具名称。
    pub tool_name: String,
    /// 结果内容。
    pub content: Vec<ContentBlock>,
    /// 调用方自定义附加信息。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    /// 是否为错误结果。
    pub is_error: bool,
    /// Unix 毫秒时间戳。
    pub timestamp: i64,
}

/// 上下文中的消息联合类型。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "role")]
pub enum Message {
    /// 用户消息。
    #[serde(rename = "user")]
    User(UserMessage),
    /// 助手消息。
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    /// 工具结果消息。
    #[serde(rename = "toolResult")]
    ToolResult(ToolResultMessage),
}

/// 工具定义。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// 工具名称。
    pub name: String,
    /// 自然语言描述。
    pub description: String,
    /// JSON Schema 参数定义。
    pub parameters: Value,
}

/// 会话上下文。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    /// 系统提示词。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// 历史消息序列。
    pub messages: Vec<Message>,
    /// 可用工具列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
}

/// AssistantMessage 流事件协议联合类型。
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantMessageEvent {
    /// 流开始。
    #[serde(rename = "start")]
    Start { partial: AssistantMessage },
    /// 文本块开始。
    #[serde(rename = "text_start")]
    TextStart { content_index: usize, partial: AssistantMessage },
    /// 文本增量。
    #[serde(rename = "text_delta")]
    TextDelta { content_index: usize, delta: String, partial: AssistantMessage },
    /// 文本块结束；原冗余 `content: String` 可从 `partial.content[content_index]` 读取。
    #[serde(rename = "text_end")]
    TextEnd { content_index: usize, partial: AssistantMessage },
    /// thinking 块开始。
    #[serde(rename = "thinking_start")]
    ThinkingStart { content_index: usize, partial: AssistantMessage },
    /// thinking 增量。
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { content_index: usize, delta: String, partial: AssistantMessage },
    /// thinking 块结束；原冗余 `content: String` 可从 `partial.content[content_index]` 读取。
    #[serde(rename = "thinking_end")]
    ThinkingEnd { content_index: usize, partial: AssistantMessage },
    /// tool call 开始。
    #[serde(rename = "toolcall_start")]
    ToolCallStart { content_index: usize, partial: AssistantMessage },
    /// tool call 参数增量。
    #[serde(rename = "toolcall_delta")]
    ToolCallDelta { content_index: usize, delta: String, partial: AssistantMessage },
    /// tool call 结束；原冗余 `tool_call: ToolCall` 可从 `partial.content[content_index]` 读取。
    #[serde(rename = "toolcall_end")]
    ToolCallEnd { content_index: usize, partial: AssistantMessage },
    /// 流正常结束。
    #[serde(rename = "done")]
    Done { reason: StopReason, message: AssistantMessage },
}

/// 统一模型元数据接口。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    /// 模型 id。
    pub id: String,
    /// 模型展示名。
    pub name: String,
    /// API 标识。
    pub api: Api,
    /// Provider 标识。
    pub provider: Provider,
    /// Provider 基础 URL。
    pub base_url: String,
    /// 是否支持 reasoning。
    pub reasoning: bool,
    /// 思考档位映射。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub thinking_level_map: ThinkingLevelMap,
    /// 模型可接收的输入类型集合。
    pub input: Vec<String>,
    /// 计费配置。
    pub cost: ModelCost,
    /// 上下文窗口大小。
    pub context_window: u64,
    /// 单次回复最大输出 token 数。
    pub max_tokens: u64,
    /// 模型自带请求头。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// 兼容性覆盖，Provider 具体解释。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
}

/// 模型计费配置。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    /// 输入 token 单价。
    pub input: f64,
    /// 输出 token 单价。
    pub output: f64,
    /// 缓存命中 token 单价。
    pub cache_read: f64,
    /// 缓存写入 token 单价。
    pub cache_write: f64,
}

/// 创建空 Usage 对象。
pub fn empty_usage() -> Usage {
    Usage::default()
}
