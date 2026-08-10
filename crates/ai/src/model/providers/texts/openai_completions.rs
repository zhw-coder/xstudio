//! OpenAI Chat Completions API 文本 Provider。
//! 本实现使用标准 `POST /chat/completions` SSE 协议，不依赖 OpenAI SDK。
//! 对兼容服务额外解析其 `reasoning_content` 与 `reasoning_details` 扩展字段。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde_json::{json, Map, Value};
use std::{collections::HashMap, time::Duration};

use crate::model::{
    api_registry::{ApiProvider, AssistantMessageEventSink},
    stream::{apply_provider_payload_callback, notify_provider_response_callback},
    types::*,
    utils::json_parse::parse_streaming_json_value,
};

/// OpenAI Chat Completions 文本 Provider。
#[derive(Debug, Default, Clone)]
pub struct OpenAICompletionsProvider;

impl OpenAICompletionsProvider {
    /// OpenAI Chat Completions API 标识。
    pub const API: &'static str = "OpenAI-Completions";
}

#[async_trait]
impl ApiProvider for OpenAICompletionsProvider {
    /// 调用 OpenAI Models API 获取可用模型列表。
    async fn models(
        &self,
        provider: &str,
        base_url: &str,
        options: &StreamOptions,
        auth: &Auth,
    ) -> Result<Vec<Model>, StreamError> {
        list_openai_completions_models(provider, base_url, options, auth).await
    }

    /// 以 OpenAI Chat Completions SSE 流调用模型。
    async fn stream(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        stream_openai_completions(model, context, options, auth, sink).await
    }

    /// 简化入口复用完整入口。
    async fn stream_simple(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        stream_openai_completions(model, context, options, auth, sink).await
    }
}

/// 执行一次 OpenAI Chat Completions 流式请求并发送内部事件。
pub async fn stream_openai_completions(
    model: &Model,
    context: Context,
    options: &StreamOptions,
    auth: &Auth,
    sink: &mut dyn AssistantMessageEventSink,
) -> Result<AssistantMessage, StreamError> {
    async {
        let api_key = resolve_api_key(model, auth)?;
        let payload = apply_provider_payload_callback(options, build_payload(model, context, options), model).await?;
        let response = send_request(model, options, auth, &api_key, payload).await?;
        notify_provider_response_callback(options, provider_response(&response), model).await?;

        let mut output = sink.emit(AssistantMessageEvent::Start { partial: empty_partial_message(model) }).await?;
        if output.stop_reason == StopReason::Aborted {
            return Ok(output);
        }
        output = process_sse_response(response, model, output, sink).await?;
        if output.stop_reason == StopReason::Stop
            && output.content.iter().any(|block| matches!(block, ContentBlock::ToolCall(_)))
        {
            output.stop_reason = StopReason::ToolUse;
        }
        output = sink.emit(AssistantMessageEvent::Done { reason: output.stop_reason, message: output }).await?;
        Ok::<AssistantMessage, StreamError>(output)
    }
    .await
}

/// 调用 OpenAI `GET /models` 并转换为内部 Model 元数据。
pub async fn list_openai_completions_models(
    provider: &str,
    base_url: &str,
    options: &StreamOptions,
    auth: &Auth,
) -> Result<Vec<Model>, StreamError> {
    async {
        let api_key = auth.api_key.as_deref().ok_or(OpenAICompletionsError::MissingApiKey)?;
        let client = reqwest::Client::builder()
            .timeout(options.timeout_ms.map(Duration::from_millis).unwrap_or(Duration::from_secs(60)))
            .build()?;
        let response = client
            .get(format!("{}/models", base_url.trim_end_matches('/')))
            .headers(build_base_headers(options, auth, api_key)?)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(OpenAICompletionsError::Upstream(format!(
                "{} {}",
                status,
                response.text().await.unwrap_or_default()
            )));
        }
        let body: Value = response.json().await?;
        let mut models = body
            .get("data")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(|item| model_from_value(item, base_url, provider)).collect::<Vec<_>>())
            .unwrap_or_default();
        models.sort_by(|left, right| left.id.cmp(&right.id));
        Ok::<Vec<Model>, OpenAICompletionsError>(models)
    }
    .await
    .map_err(StreamError::from)
}

/// Provider 内部错误。
#[derive(Debug, thiserror::Error)]
enum OpenAICompletionsError {
    /// 缺失 API Key。
    #[error("OpenAI API key is required. Set OPENAI_API_KEY or pass auth.api_key")]
    MissingApiKey,
    /// HTTP 层错误。
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    /// Header 构造错误。
    #[error("Invalid header: {0}")]
    InvalidHeader(String),
    /// SSE JSON 解析错误。
    #[error("SSE JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// 上游返回错误。
    #[error("OpenAI error: {0}")]
    Upstream(String),
}

impl From<OpenAICompletionsError> for StreamError {
    /// 将 Provider 内部错误向上转换为通用流错误。
    fn from(error: OpenAICompletionsError) -> Self {
        Self::Stream(error.to_string())
    }
}

/// 构造标准 OpenAI Chat Completions 请求 payload。
fn build_payload(model: &Model, context: Context, options: &StreamOptions) -> Value {
    let Context { system_prompt, messages, tools } = context;
    let mut payload = Map::new();
    payload.insert("model".into(), json!(model.id));
    payload.insert("messages".into(), convert_messages(system_prompt, messages));
    payload.insert("stream".into(), json!(true));
    payload.insert("stream_options".into(), json!({ "include_usage": true }));
    if let Some(max_tokens) = options.max_tokens {
        payload.insert("max_completion_tokens".into(), json!(max_tokens));
    }
    if let Some(temperature) = options.temperature {
        payload.insert("temperature".into(), json!(temperature));
    }
    if model.reasoning {
        if let Some(reasoning) = options.reasoning {
            let effort =
                model.thinking_level_map.get(&reasoning).cloned().flatten().unwrap_or_else(|| reasoning.to_string());
            payload.insert("reasoning_effort".into(), json!(effort));
        }
    }
    if !tools.is_empty() {
        payload.insert("tools".into(), convert_tools(tools));
    }
    Value::Object(payload)
}

/// 将内部上下文转换为标准 Chat Completions messages。
fn convert_messages(system_prompt: Option<String>, messages: Vec<Message>) -> Value {
    let mut output = Vec::new();
    if let Some(system_prompt) = system_prompt {
        output.push(json!({ "role": "system", "content": system_prompt }));
    }
    for message in messages {
        match message {
            Message::User(message) => {
                output.push(json!({ "role": "user", "content": convert_user_content(message.content) }))
            }
            Message::Assistant(message) => output.push(convert_assistant_message(message)),
            Message::ToolResult(message) => output.push(json!({
                "role": "tool",
                "tool_call_id": message.tool_call_id.split('|').next().unwrap_or(&message.tool_call_id),
                "content": tool_result_text(message),
            })),
        }
    }
    Value::Array(output)
}

/// 转换用户消息内容。
fn convert_user_content(content: UserContent) -> Value {
    match content {
        UserContent::Text(text) => Value::String(text),
        UserContent::Blocks(blocks) => Value::Array(
            blocks
                .into_iter()
                .filter_map(|block| match block {
                    ContentBlock::Text(text) => Some(json!({ "type": "text", "text": text.text })),
                    ContentBlock::Image(image) => Some(json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{};base64,{}", image.mime_type, image.data) },
                    })),
                    _ => None,
                })
                .collect(),
        ),
    }
}

/// 转换助手历史消息及其中的工具调用。
fn convert_assistant_message(message: AssistantMessage) -> Value {
    let mut text = String::new();
    let mut reasoning_content = String::new();
    let mut reasoning_details = Vec::new();
    let mut tool_calls = Vec::new();
    for block in message.content {
        match block {
            ContentBlock::Text(content) => text.push_str(&content.text),
            ContentBlock::Thinking(content) => {
                reasoning_content.push_str(&content.thinking);
                if let Some(signature) = content.thinking_signature {
                    if let Ok(details) = serde_json::from_str(&signature) {
                        reasoning_details.push(details);
                    }
                }
            }
            ContentBlock::ToolCall(tool_call) => tool_calls.push(json!({
                "id": tool_call.id.split('|').next().unwrap_or(&tool_call.id),
                "type": "function",
                "function": { "name": tool_call.name, "arguments": Value::Object(tool_call.arguments).to_string() },
            })),
            ContentBlock::Image(_) => {}
        }
    }
    let mut message = Map::new();
    message.insert("role".into(), json!("assistant"));
    message.insert("content".into(), if text.is_empty() { Value::Null } else { Value::String(text) });
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    if !reasoning_content.is_empty() {
        message.insert("reasoning_content".into(), Value::String(reasoning_content));
    }
    if !reasoning_details.is_empty() {
        message.insert("reasoning_details".into(), Value::Array(reasoning_details));
    }
    Value::Object(message)
}

/// 转换工具定义为标准 Chat Completions function tools。
fn convert_tools(tools: Vec<Tool>) -> Value {
    Value::Array(
        tools
            .into_iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": { "name": tool.name, "description": tool.description, "parameters": tool.parameters },
                })
            })
            .collect(),
    )
}

/// 将工具结果压缩为 tool message 内容。
fn tool_result_text(message: ToolResultMessage) -> String {
    message
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text),
            ContentBlock::Image(image) => Some(format!("[image:{};base64,{}]", image.mime_type, image.data)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 发送 OpenAI Chat Completions HTTP 请求。
async fn send_request(
    model: &Model,
    options: &StreamOptions,
    auth: &Auth,
    api_key: &str,
    payload: Value,
) -> Result<reqwest::Response, OpenAICompletionsError> {
    let client = reqwest::Client::builder()
        .timeout(options.timeout_ms.map(Duration::from_millis).unwrap_or(Duration::from_secs(600)))
        .build()?;
    let mut headers = build_headers(model, options, auth, api_key)?;
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    let response = client
        .post(format!("{}/chat/completions", model.base_url.trim_end_matches('/')))
        .headers(headers)
        .json(&payload)
        .send()
        .await?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(OpenAICompletionsError::Upstream(format!(
            "{} {}",
            status,
            response.text().await.unwrap_or_default()
        )));
    }
    Ok(response)
}

/// 构造带模型私有请求头的请求头。
fn build_headers(
    model: &Model,
    options: &StreamOptions,
    auth: &Auth,
    api_key: &str,
) -> Result<HeaderMap, OpenAICompletionsError> {
    let mut headers = build_base_headers(options, auth, api_key)?;
    for (key, value) in &model.headers {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|error| OpenAICompletionsError::InvalidHeader(error.to_string()))?;
        let value =
            HeaderValue::from_str(value).map_err(|error| OpenAICompletionsError::InvalidHeader(error.to_string()))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

/// 构造 OpenAI 共享认证请求头。
fn build_base_headers(
    options: &StreamOptions,
    auth: &Auth,
    api_key: &str,
) -> Result<HeaderMap, OpenAICompletionsError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|error| OpenAICompletionsError::InvalidHeader(error.to_string()))?,
    );
    for (key, value) in options.headers.iter().chain(auth.headers.iter()) {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|error| OpenAICompletionsError::InvalidHeader(error.to_string()))?;
        let value =
            HeaderValue::from_str(value).map_err(|error| OpenAICompletionsError::InvalidHeader(error.to_string()))?;
        headers.insert(name, value);
    }
    if let Some(session_id) = &options.session_id {
        headers.insert(
            "x-client-request-id",
            HeaderValue::from_str(session_id)
                .map_err(|error| OpenAICompletionsError::InvalidHeader(error.to_string()))?,
        );
    }
    Ok(headers)
}

/// 解析 API Key，优先使用认证信息，其次使用环境变量。
fn resolve_api_key(model: &Model, auth: &Auth) -> Result<String, OpenAICompletionsError> {
    if let Some(api_key) = &auth.api_key {
        return Ok(api_key.clone());
    }
    let provider_key = format!("{}_API_KEY", model.provider.to_ascii_uppercase().replace('-', "_"));
    std::env::var(provider_key)
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| OpenAICompletionsError::MissingApiKey)
}

/// 将 OpenAI model 条目转换为内部 Model 元数据。
fn model_from_value(value: &Value, base_url: &str, provider: &str) -> Option<Model> {
    let id = value.get("id").and_then(Value::as_str)?.to_string();
    Some(Model {
        id: id.clone(),
        name: id,
        api: OpenAICompletionsProvider::API.to_string(),
        provider: provider.to_string(),
        base_url: base_url.to_string(),
        reasoning: true,
        thinking_level_map: HashMap::new(),
        input: vec!["text".to_string(), "image".to_string()],
        cost: ModelCost::default(),
        context_window: 131072,
        max_tokens: 0,
        headers: HashMap::new(),
        compat: None,
    })
}

/// 提取 Provider HTTP 响应信息。
fn provider_response(response: &reqwest::Response) -> ProviderResponse {
    ProviderResponse {
        status: response.status().as_u16(),
        headers: response
            .headers()
            .iter()
            .filter_map(|(key, value)| value.to_str().ok().map(|value| (key.to_string(), value.to_string())))
            .collect(),
    }
}

/// 处理 Chat Completions SSE 响应。
async fn process_sse_response(
    response: reqwest::Response,
    model: &Model,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
) -> Result<AssistantMessage, StreamError> {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut state = StreamState::default();
    while let Some(chunk) = stream.next().await {
        for event in process_sse_chunk(chunk.map_err(OpenAICompletionsError::from)?, &mut buffer)? {
            output = process_chunk(event, model, output, sink, &mut state).await?;
        }
    }
    finish_open_blocks(output, sink, &mut state).await
}

/// Chat Completions 流处理状态。
#[derive(Default)]
struct StreamState {
    /// 文本内容块下标。
    text_index: Option<usize>,
    /// 兼容服务扩展的 reasoning 内容块下标。
    thinking_index: Option<usize>,
    /// 上游 tool_calls.index 到内部内容块下标的映射。
    tool_indices: HashMap<usize, usize>,
    /// 各工具调用的累积 JSON 参数。
    tool_arguments: HashMap<usize, String>,
}

/// 按 SSE 行协议切分 chunk。
fn process_sse_chunk(chunk: Bytes, buffer: &mut String) -> Result<Vec<Value>, OpenAICompletionsError> {
    let mut events = Vec::new();
    buffer.push_str(&String::from_utf8_lossy(&chunk));
    while let Some(position) = buffer.find('\n') {
        let line = buffer[..position].trim_end_matches('\r').to_string();
        buffer.drain(..=position);
        let Some(data) = line.trim().strip_prefix("data: ") else { continue };
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        events.push(serde_json::from_str(data)?);
    }
    Ok(events)
}

/// 处理一个 Chat Completions SSE chunk。
async fn process_chunk(
    event: Value,
    model: &Model,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    if let Some(id) = event.get("id").and_then(Value::as_str) {
        output.response_id = Some(id.to_string());
    }
    if let Some(response_model) = event.get("model").and_then(Value::as_str) {
        output.response_model = Some(response_model.to_string());
    }
    if let Some(usage) = event.get("usage") {
        set_usage(usage, model, &mut output.usage);
    }
    let Some(choice) = event.get("choices").and_then(Value::as_array).and_then(|choices| choices.first()) else {
        return Ok(output);
    };
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        output.stop_reason = match reason {
            "length" => StopReason::Length,
            "tool_calls" | "function_call" => StopReason::ToolUse,
            "content_filter" => StopReason::Error,
            _ => StopReason::Stop,
        };
    }
    let delta = choice.get("delta").unwrap_or(&Value::Null);
    if let Some(content) = delta.get("content").and_then(Value::as_str) {
        output = handle_text_delta(content, output, sink, state).await?;
    }
    if let Some(reasoning_content) = delta.get("reasoning_content").and_then(Value::as_str) {
        output = handle_thinking_delta(reasoning_content, output, sink, state).await?;
    }
    if let Some(reasoning_details) = delta.get("reasoning_details") {
        set_thinking_signature(reasoning_details, &mut output, state);
    }
    if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            output = handle_tool_delta(tool_call, output, sink, state).await?;
        }
    }
    Ok(output)
}

/// 追加一个文本增量，必要时创建文本块。
async fn handle_text_delta(
    delta: &str,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    let index = match state.text_index {
        Some(index) => index,
        None => {
            output.content.push(ContentBlock::Text(TextContent { text: String::new(), text_signature: None }));
            let index = output.content.len() - 1;
            state.text_index = Some(index);
            output = sink.emit(AssistantMessageEvent::TextStart { content_index: index, partial: output }).await?;
            index
        }
    };
    if let Some(ContentBlock::Text(text)) = output.content.get_mut(index) {
        text.text.push_str(delta);
    }
    sink.emit(AssistantMessageEvent::TextDelta { content_index: index, delta: delta.to_string(), partial: output })
        .await
}

/// 追加兼容服务输出的 reasoning 内容，必要时创建 thinking 块。
async fn handle_thinking_delta(
    delta: &str,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    let index = match state.thinking_index {
        Some(index) => index,
        None => {
            output.content.push(ContentBlock::Thinking(ThinkingContent {
                thinking: String::new(),
                thinking_signature: None,
                redacted: None,
            }));
            let index = output.content.len() - 1;
            state.thinking_index = Some(index);
            output = sink.emit(AssistantMessageEvent::ThinkingStart { content_index: index, partial: output }).await?;
            index
        }
    };
    if let Some(ContentBlock::Thinking(thinking)) = output.content.get_mut(index) {
        thinking.thinking.push_str(delta);
    }
    sink.emit(AssistantMessageEvent::ThinkingDelta { content_index: index, delta: delta.to_string(), partial: output })
        .await
}

/// 保存兼容服务提供的 reasoning 详情，以便下一轮请求原样回传。
fn set_thinking_signature(reasoning_details: &Value, output: &mut AssistantMessage, state: &StreamState) {
    let Some(index) = state.thinking_index else { return };
    let Some(ContentBlock::Thinking(thinking)) = output.content.get_mut(index) else { return };
    thinking.thinking_signature = Some(reasoning_details.to_string());
}

/// 处理一个工具调用参数增量。
async fn handle_tool_delta(
    tool_call: &Value,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    let upstream_index = tool_call.get("index").and_then(Value::as_u64).unwrap_or_default() as usize;
    let index = match state.tool_indices.get(&upstream_index) {
        Some(index) => *index,
        None => {
            let function = tool_call.get("function").unwrap_or(&Value::Null);
            output.content.push(ContentBlock::ToolCall(ToolCall {
                id: tool_call.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                name: function.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                arguments: Map::new(),
                thought_signature: None,
            }));
            let index = output.content.len() - 1;
            state.tool_indices.insert(upstream_index, index);
            output = sink.emit(AssistantMessageEvent::ToolCallStart { content_index: index, partial: output }).await?;
            index
        }
    };
    let arguments_delta = tool_call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or_default();
    let partial_arguments = state.tool_arguments.entry(upstream_index).or_default();
    partial_arguments.push_str(arguments_delta);
    if let Some(ContentBlock::ToolCall(tool)) = output.content.get_mut(index) {
        if let Value::Object(arguments) = parse_streaming_json_value(Some(partial_arguments)) {
            tool.arguments = arguments;
        }
    }
    sink.emit(AssistantMessageEvent::ToolCallDelta {
        content_index: index,
        delta: arguments_delta.to_string(),
        partial: output,
    })
    .await
}

/// 结束所有尚未关闭的内容块。
async fn finish_open_blocks(
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    if let Some(index) = state.text_index.take() {
        output = sink.emit(AssistantMessageEvent::TextEnd { content_index: index, partial: output }).await?;
    }
    if let Some(index) = state.thinking_index.take() {
        output = sink.emit(AssistantMessageEvent::ThinkingEnd { content_index: index, partial: output }).await?;
    }
    let mut indices = state.tool_indices.values().copied().collect::<Vec<_>>();
    indices.sort_unstable();
    for index in indices {
        output = sink.emit(AssistantMessageEvent::ToolCallEnd { content_index: index, partial: output }).await?;
    }
    Ok(output)
}

/// 从 Chat Completions usage 填充内部用量并计算费用。
fn set_usage(value: &Value, model: &Model, usage: &mut Usage) {
    let input_tokens = value.get("prompt_tokens").and_then(Value::as_u64).unwrap_or_default();
    let output_tokens = value.get("completion_tokens").and_then(Value::as_u64).unwrap_or_default();
    let cached_tokens =
        value.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64).unwrap_or_default();
    usage.input = input_tokens.saturating_sub(cached_tokens);
    usage.output = output_tokens;
    usage.cache_read = cached_tokens;
    usage.cache_write = 0;
    usage.total_tokens = value.get("total_tokens").and_then(Value::as_u64).unwrap_or(input_tokens + output_tokens);
    usage.cost.input = model.cost.input / 1_000_000.0 * usage.input as f64;
    usage.cost.output = model.cost.output / 1_000_000.0 * usage.output as f64;
    usage.cost.cache_read = model.cost.cache_read / 1_000_000.0 * usage.cache_read as f64;
    usage.cost.cache_write = 0.0;
    usage.cost.total = usage.cost.input + usage.cost.output + usage.cost.cache_read;
}

/// 构造客户端维护的空 partial AssistantMessage。
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
        timestamp: now_millis(),
    }
}
