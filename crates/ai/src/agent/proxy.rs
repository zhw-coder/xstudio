//! 经由代理服务器调用 LLM Provider 的 stream 函数实现：服务器持有真实凭据，客户端通过该函数把请求转发到代理。
//! 服务器在转发期间会剥离 delta 事件中的 `partial` 字段以减小带宽，本模块负责在客户端按事件类型重建出
//! 完整的 `AssistantMessage` 流，使下游观察者得到与 `stream_simple` 一致的事件序列。

use std::collections::HashMap;

use bytes::Bytes;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::model::{
    api_registry::AssistantMessageEventSink,
    types::{
        empty_usage, AssistantMessage, AssistantMessageEvent, CacheRetention, ContentBlock, Context, Model, StopReason,
        StreamError, TextContent, ThinkingBudgets, ThinkingContent, ToolCall, Transport, Usage,
    },
    utils::json_parse::parse_streaming_json_value,
};

/// 代理事件类型：服务器在 SSE 流中按这些事件下发数据。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProxyAssistantMessageEvent {
    /// 流开始事件，仅起占位作用。
    #[serde(rename = "start")]
    Start,
    /// 文本块开始。
    #[serde(rename = "text_start")]
    TextStart { content_index: usize },
    /// 文本增量。
    #[serde(rename = "text_delta")]
    TextDelta { content_index: usize, delta: String },
    /// 文本块结束。
    #[serde(rename = "text_end")]
    TextEnd { content_index: usize, content_signature: Option<String> },
    /// thinking 内容块开始。
    #[serde(rename = "thinking_start")]
    ThinkingStart { content_index: usize },
    /// thinking 内容增量。
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { content_index: usize, delta: String },
    /// thinking 内容块结束。
    #[serde(rename = "thinking_end")]
    ThinkingEnd { content_index: usize, content_signature: Option<String> },
    /// tool call 内容块开始。
    #[serde(rename = "toolcall_start")]
    ToolCallStart { content_index: usize, id: String, tool_name: String },
    /// tool call 参数 JSON 的增量字符串。
    #[serde(rename = "toolcall_delta")]
    ToolCallDelta { content_index: usize, delta: String },
    /// tool call 内容块结束。
    #[serde(rename = "toolcall_end")]
    ToolCallEnd { content_index: usize },
    /// 流正常结束。
    #[serde(rename = "done")]
    Done { reason: StopReason, usage: Usage },
}

/// 可序列化的 stream 选项子集。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProxySerializableStreamOptions {
    /// 采样温度。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// 最大输出 token。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// 思考档位。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<crate::model::types::ThinkingLevel>,
    /// 缓存保留策略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_retention: Option<CacheRetention>,
    /// Provider session id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// 自定义请求头。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// 调用方元数据。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// 传输模式。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    /// thinking token 预算。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budgets: Option<ThinkingBudgets>,
    /// 最大重试延迟。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retry_delay_ms: Option<u64>,
}

/// 代理 stream 选项。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStreamOptions {
    /// Bearer token。
    pub auth_token: String,
    /// 代理服务器根 URL。
    pub proxy_url: String,
    /// 可序列化 stream 选项。
    #[serde(flatten)]
    pub serializable: ProxySerializableStreamOptions,
    /// 运行时取消令牌；对应 TS 版 `AbortSignal`，不参与代理请求序列化。
    #[serde(skip)]
    pub cancellation_token: Option<CancellationToken>,
}

impl PartialEq for ProxyStreamOptions {
    /// 只比较可序列化请求字段；取消令牌是运行时控制句柄，不属于请求语义。
    fn eq(&self, other: &Self) -> bool {
        self.auth_token == other.auth_token
            && self.proxy_url == other.proxy_url
            && self.serializable == other.serializable
    }
}

/// stream_proxy 错误。
#[derive(Debug, thiserror::Error)]
pub enum ProxyStreamError {
    /// HTTP 请求失败。
    #[error("Proxy request failed: {0}")]
    Request(#[from] reqwest::Error),
    /// 代理返回错误响应。
    #[error("Proxy error: {0}")]
    Response(String),
    /// 代理事件 JSON 解析失败。
    #[error("Proxy event parse failed: {0}")]
    EventParse(#[from] serde_json::Error),
    /// 代理事件协议非法。
    #[error("Proxy protocol error: {0}")]
    Protocol(String),
    /// 调用方主动取消。
    #[error("Proxy stream aborted")]
    Aborted,
}

impl From<ProxyStreamError> for StreamError {
    /// 将代理 Provider 错误向上转换为通用流错误。
    fn from(error: ProxyStreamError) -> Self {
        StreamError::Stream(error.to_string())
    }
}

/// 把 `ProxyStreamOptions` 中的可序列化字段挑出来构造 POST body 的子对象。
pub fn build_proxy_request_options(options: &ProxyStreamOptions) -> &ProxySerializableStreamOptions {
    &options.serializable
}

/// 代理 stream 入口：发起 POST `/api/stream` 请求，按 SSE 形式读取代理服务器响应。
pub async fn stream_proxy(
    model: Model,
    context: Context,
    options: ProxyStreamOptions,
    sink: &mut dyn AssistantMessageEventSink,
) -> Result<AssistantMessage, StreamError> {
    let mut partial = empty_partial_message(&model);
    let client = reqwest::Client::new();
    let url = format!("{}/api/stream", options.proxy_url.trim_end_matches('/'));
    let cancellation_token = &options.cancellation_token;
    if is_cancelled(cancellation_token) {
        return Err(ProxyStreamError::Aborted.into());
    }
    let request = client.post(url).bearer_auth(&options.auth_token).json(&serde_json::json!({
        "model": model,
        "context": context,
        "options": build_proxy_request_options(&options),
    }));
    let response = if let Some(token) = cancellation_token {
        tokio::select! {
            response = request.send() => response.map_err(ProxyStreamError::from)?,
            _ = token.cancelled() => return Err(ProxyStreamError::Aborted.into()),
        }
    } else {
        request.send().await.map_err(ProxyStreamError::from)?
    };

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(ProxyStreamError::Response(format!("{status} {text}")).into());
    }

    let mut buffer = String::new();
    let mut bytes = response.bytes_stream();
    while let Some(chunk) = next_proxy_chunk(&mut bytes, cancellation_token).await? {
        let mut proxy_events = Vec::new();
        read_sse_chunk(chunk.map_err(ProxyStreamError::from)?, &mut buffer, |event| {
            proxy_events.push(event);
            Ok(())
        })
        .map_err(StreamError::from)?;
        for event in proxy_events {
            if let Some(message_event) = process_proxy_event(event, &mut partial)? {
                partial = sink.emit(message_event).await?;
            }
        }
    }
    Ok(partial)
}

/// 判断代理请求是否已被调用方取消。
fn is_cancelled(cancellation_token: &Option<CancellationToken>) -> bool {
    cancellation_token.as_ref().is_some_and(CancellationToken::is_cancelled)
}

/// 读取下一个 SSE 字节块，同时响应调用方取消。
async fn next_proxy_chunk(
    bytes: &mut (impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Unpin),
    cancellation_token: &Option<CancellationToken>,
) -> Result<Option<Result<Bytes, reqwest::Error>>, ProxyStreamError> {
    if let Some(token) = cancellation_token {
        tokio::select! {
            chunk = bytes.next() => Ok(chunk),
            _ = token.cancelled() => Err(ProxyStreamError::Aborted),
        }
    } else {
        Ok(bytes.next().await)
    }
}

/// 读取一个 SSE chunk 并分派 data 行。
fn read_sse_chunk(
    chunk: Bytes,
    buffer: &mut String,
    mut on_event: impl FnMut(ProxyAssistantMessageEvent) -> Result<(), ProxyStreamError>,
) -> Result<(), ProxyStreamError> {
    buffer.push_str(&String::from_utf8_lossy(&chunk));
    let mut lines = buffer.split('\n').map(ToOwned::to_owned).collect::<Vec<_>>();
    *buffer = lines.pop().unwrap_or_default();
    for line in lines {
        if let Some(data) = line.trim().strip_prefix("data: ") {
            if !data.is_empty() {
                on_event(serde_json::from_str(data)?)?;
            }
        }
    }
    Ok(())
}

/// 处理一个代理事件并重建完整的 AssistantMessageEvent。
pub fn process_proxy_event(
    proxy_event: ProxyAssistantMessageEvent,
    partial: &mut AssistantMessage,
) -> Result<Option<AssistantMessageEvent>, ProxyStreamError> {
    match proxy_event {
        ProxyAssistantMessageEvent::Start => {
            Ok(Some(AssistantMessageEvent::Start { partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::TextStart { content_index } => {
            set_content(
                partial,
                content_index,
                ContentBlock::Text(TextContent { text: String::new(), text_signature: None }),
            );
            Ok(Some(AssistantMessageEvent::TextStart { content_index, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::TextDelta { content_index, delta } => {
            let content = partial
                .content
                .get_mut(content_index)
                .ok_or_else(|| ProxyStreamError::Protocol("text_delta content index out of range".into()))?;
            let ContentBlock::Text(text) = content else {
                return Err(ProxyStreamError::Protocol("Received text_delta for non-text content".into()));
            };
            text.text.push_str(&delta);
            Ok(Some(AssistantMessageEvent::TextDelta { content_index, delta, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::TextEnd { content_index, content_signature } => {
            let content = partial
                .content
                .get_mut(content_index)
                .ok_or_else(|| ProxyStreamError::Protocol("text_end content index out of range".into()))?;
            let ContentBlock::Text(text) = content else {
                return Err(ProxyStreamError::Protocol("Received text_end for non-text content".into()));
            };
            text.text_signature = content_signature;
            Ok(Some(AssistantMessageEvent::TextEnd { content_index, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::ThinkingStart { content_index } => {
            set_content(
                partial,
                content_index,
                ContentBlock::Thinking(ThinkingContent {
                    thinking: String::new(),
                    thinking_signature: None,
                    redacted: None,
                }),
            );
            Ok(Some(AssistantMessageEvent::ThinkingStart { content_index, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::ThinkingDelta { content_index, delta } => {
            let content = partial
                .content
                .get_mut(content_index)
                .ok_or_else(|| ProxyStreamError::Protocol("thinking_delta content index out of range".into()))?;
            let ContentBlock::Thinking(thinking) = content else {
                return Err(ProxyStreamError::Protocol("Received thinking_delta for non-thinking content".into()));
            };
            thinking.thinking.push_str(&delta);
            Ok(Some(AssistantMessageEvent::ThinkingDelta { content_index, delta, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::ThinkingEnd { content_index, content_signature } => {
            let content = partial
                .content
                .get_mut(content_index)
                .ok_or_else(|| ProxyStreamError::Protocol("thinking_end content index out of range".into()))?;
            let ContentBlock::Thinking(thinking) = content else {
                return Err(ProxyStreamError::Protocol("Received thinking_end for non-thinking content".into()));
            };
            thinking.thinking_signature = content_signature;
            Ok(Some(AssistantMessageEvent::ThinkingEnd { content_index, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::ToolCallStart { content_index, id, tool_name } => {
            set_content(
                partial,
                content_index,
                ContentBlock::ToolCall(ToolCall {
                    id,
                    name: tool_name,
                    arguments: Default::default(),
                    thought_signature: None,
                }),
            );
            Ok(Some(AssistantMessageEvent::ToolCallStart { content_index, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::ToolCallDelta { content_index, delta } => {
            let content = partial
                .content
                .get_mut(content_index)
                .ok_or_else(|| ProxyStreamError::Protocol("toolcall_delta content index out of range".into()))?;
            let ContentBlock::ToolCall(tool_call) = content else {
                return Err(ProxyStreamError::Protocol("Received toolcall_delta for non-toolCall content".into()));
            };
            let mut partial_json = serde_json::to_string(&tool_call.arguments).unwrap_or_default();
            if partial_json == "{}" {
                partial_json.clear();
            }
            partial_json.push_str(&delta);
            if let Value::Object(args) = parse_streaming_json_value(Some(&partial_json)) {
                tool_call.arguments = args;
            }
            Ok(Some(AssistantMessageEvent::ToolCallDelta { content_index, delta, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::ToolCallEnd { content_index } => {
            let content = partial
                .content
                .get(content_index)
                .ok_or_else(|| ProxyStreamError::Protocol("toolcall_end content index out of range".into()))?;
            let ContentBlock::ToolCall(_) = content else {
                return Err(ProxyStreamError::Protocol("Received toolcall_end for non-toolCall content".into()));
            };
            Ok(Some(AssistantMessageEvent::ToolCallEnd { content_index, partial: std::mem::take(partial) }))
        }
        ProxyAssistantMessageEvent::Done { reason, usage } => {
            partial.stop_reason = reason;
            partial.usage = usage;
            Ok(Some(AssistantMessageEvent::Done { reason, message: std::mem::take(partial) }))
        }
    }
}

/// 构造客户端本地维护的空 partial AssistantMessage。
fn empty_partial_message(model: &Model) -> AssistantMessage {
    AssistantMessage {
        content: vec![],
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        diagnostics: vec![],
        usage: empty_usage(),
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: crate::model::types::now_millis(),
    }
}

/// 在指定下标设置 content block，必要时用空文本块补齐中间空位。
fn set_content(partial: &mut AssistantMessage, index: usize, content: ContentBlock) {
    while partial.content.len() <= index {
        partial.content.push(ContentBlock::Text(TextContent { text: String::new(), text_signature: None }));
    }
    partial.content[index] = content;
}
