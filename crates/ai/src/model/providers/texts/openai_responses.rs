//! OpenAI Responses API 文本 Provider。
//! 本实现直接使用真实 OpenAI 协议：`POST /responses`、`stream: true`
//! 与 SSE 事件，不依赖 OpenAI SDK。

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

/// OpenAI Responses 文本 Provider。
#[derive(Debug, Default, Clone)]
pub struct OpenAIResponsesProvider;

impl OpenAIResponsesProvider {
    /// OpenAI Responses API 标识。
    pub const API: &'static str = "OpenAI-Responses";
}

#[async_trait]
impl ApiProvider for OpenAIResponsesProvider {
    /// 调用 OpenAI Models API 获取可用模型列表。
    async fn models(
        &self,
        provider: &str,
        base_url: &str,
        options: &StreamOptions,
        auth: &Auth,
    ) -> Result<Vec<Model>, StreamError> {
        list_openai_response_models(provider, base_url, options, auth).await
    }

    /// 以 OpenAI Responses SSE 流调用模型。
    async fn stream(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        stream_openai_responses(model, context, options, auth, sink).await
    }

    /// 简化入口当前复用完整入口，保持实现精简。
    async fn stream_simple(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        stream_openai_responses(model, context, options, auth, sink).await
    }
}

/// 执行一次 OpenAI Responses 流式请求并发送内部事件。
pub async fn stream_openai_responses(
    model: &Model,
    context: Context,
    options: &StreamOptions,
    auth: &Auth,
    sink: &mut dyn AssistantMessageEventSink,
) -> Result<AssistantMessage, StreamError> {
    async {
        let mut output = empty_partial_message(model);
        let api_key = resolve_api_key(model, auth)?;
        let mut payload = build_payload(model, context, options);
        payload = apply_provider_payload_callback(options, payload, model).await?;

        let response = send_request(model, options, auth, &api_key, payload).await?;
        notify_provider_response_callback(options, provider_response(&response), model).await?;
        output = sink.emit(AssistantMessageEvent::Start { partial: output }).await?;
        if output.stop_reason == StopReason::Aborted {
            return Ok::<AssistantMessage, StreamError>(output);
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
pub async fn list_openai_response_models(
    provider: &str,
    base_url: &str,
    options: &StreamOptions,
    auth: &Auth,
) -> Result<Vec<Model>, StreamError> {
    async {
        let api_key = auth.api_key.as_deref().ok_or(OpenAIResponsesError::MissingApiKey)?;
        let client = reqwest::Client::builder()
            .timeout(options.timeout_ms.map(Duration::from_millis).unwrap_or(Duration::from_secs(60)))
            .build()?;
        let url = format!("{}/models", base_url.trim_end_matches('/'));
        let headers = build_models_headers(options, auth, api_key)?;
        let response = client.get(url).headers(headers).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(OpenAIResponsesError::Upstream(format!("{} {}", status, body)));
        }
        let body: Value = response.json().await?;
        let mut models = body
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items.iter().filter_map(|item| openai_model_from_value(item, base_url, provider)).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        models.sort_by(|left, right| left.id.cmp(&right.id));
        Ok::<Vec<Model>, OpenAIResponsesError>(models)
    }
    .await
    .map_err(StreamError::from)
}

/// Provider 内部错误。
#[derive(Debug, thiserror::Error)]
enum OpenAIResponsesError {
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

impl From<OpenAIResponsesError> for StreamError {
    /// 将 OpenAI Responses Provider 内部错误向上转换为通用流错误。
    fn from(error: OpenAIResponsesError) -> Self {
        StreamError::Stream(error.to_string())
    }
}

/// 构造 OpenAI Responses 请求 payload。
fn build_payload(model: &Model, context: Context, options: &StreamOptions) -> Value {
    let Context { system_prompt, messages, tools } = context;
    let mut payload = Map::new();
    payload.insert("model".into(), json!(model.id));
    payload.insert("input".into(), convert_messages(model, system_prompt, messages));
    payload.insert("stream".into(), json!(true));
    payload.insert("store".into(), json!(false));

    if let Some(max_tokens) = options.max_tokens {
        payload.insert("max_output_tokens".into(), json!(max_tokens));
    }
    if let Some(temperature) = options.temperature {
        payload.insert("temperature".into(), json!(temperature));
    }
    let mut response_tools = convert_tools(tools);
    if let Some(Value::String(compat)) = &model.compat {
        if let Ok(Value::Object(compat)) = serde_json::from_str(compat) {
            response_tools.extend(compat.get("tools").and_then(Value::as_array).into_iter().flatten().cloned());
        }
    }
    if !response_tools.is_empty() {
        payload.insert("tools".into(), Value::Array(response_tools));
    }
    if options.cache_retention != Some(CacheRetention::None) {
        if let Some(session_id) = &options.session_id {
            payload.insert("prompt_cache_key".into(), json!(session_id));
        }
    }
    if model.reasoning {
        if let Some(reasoning) = &options.reasoning {
            let effort =
                model.thinking_level_map.get(reasoning).cloned().flatten().unwrap_or_else(|| reasoning.to_string());
            payload.insert("reasoning".into(), json!({ "effort": effort, "summary": "auto" }));
        }
    }
    if !options.metadata.is_empty() {
        payload.insert("metadata".into(), Value::Object(options.metadata.clone()));
    }

    Value::Object(payload)
}

/// 将内部消息转换为 OpenAI `input`。
fn convert_messages(model: &Model, system_prompt: Option<String>, messages: Vec<Message>) -> Value {
    let mut input = Vec::new();
    if let Some(system_prompt) = system_prompt {
        input.push(json!({
            "role": if model.reasoning { "developer" } else { "system" },
            "content": system_prompt,
        }));
    }
    for message in messages {
        match message {
            Message::User(message) => {
                input.push(json!({ "role": "user", "content": convert_user_content(message.content) }))
            }
            Message::Assistant(message) => input.extend(convert_assistant_message(message)),
            Message::ToolResult(message) => input.push(json!({
                "type": "function_call_output",
                "call_id": message.tool_call_id.split('|').next().unwrap_or(&message.tool_call_id),
                "output": tool_result_text(message),
            })),
        }
    }
    Value::Array(input)
}

/// 转换用户消息内容。
fn convert_user_content(content: UserContent) -> Value {
    match content {
        UserContent::Text(text) => json!([{ "type": "input_text", "text": text }]),
        UserContent::Blocks(blocks) => Value::Array(
            blocks
                .into_iter()
                .filter_map(|block| match block {
                    ContentBlock::Text(text) => Some(json!({ "type": "input_text", "text": text.text })),
                    ContentBlock::Image(image) => Some(json!({
                        "type": "input_image",
                        "detail": "auto",
                        "image_url": format!("data:{};base64,{}", image.mime_type, image.data),
                    })),
                    _ => None,
                })
                .collect(),
        ),
    }
}

/// 转换助手历史消息。
fn convert_assistant_message(message: AssistantMessage) -> Vec<Value> {
    let mut output = Vec::new();
    for block in message.content {
        match block {
            ContentBlock::Text(text) => output.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text.text }],
            })),
            ContentBlock::ToolCall(tool_call) => output.push(json!({
                "type": "function_call",
                "call_id": tool_call.id.split('|').next().unwrap_or(&tool_call.id),
                "name": tool_call.name,
                "arguments": Value::Object(tool_call.arguments).to_string(),
            })),
            ContentBlock::Thinking(thinking) => {
                if let Some(signature) = thinking.thinking_signature {
                    if let Ok(value) = serde_json::from_str::<Value>(&signature) {
                        output.push(value);
                    }
                }
            }
            ContentBlock::Image(_) => {}
        }
    }
    output
}

/// 转换工具定义为 OpenAI function tool。
fn convert_tools(tools: Vec<Tool>) -> Vec<Value> {
    tools
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
                "strict": false,
            })
        })
        .collect()
}

/// 将工具结果压缩为 `function_call_output.output` 字符串。
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

/// 发送真实 OpenAI Responses HTTP 请求。
async fn send_request(
    model: &Model,
    options: &StreamOptions,
    auth: &Auth,
    api_key: &str,
    payload: Value,
) -> Result<reqwest::Response, OpenAIResponsesError> {
    let client = reqwest::Client::builder()
        .timeout(options.timeout_ms.map(Duration::from_millis).unwrap_or(Duration::from_secs(600)))
        .build()?;
    let url = format!("{}/responses", model.base_url.trim_end_matches('/'));
    let mut headers = build_headers(model, options, auth, api_key)?;
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    let response = client.post(url).headers(headers).json(&payload).send().await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(OpenAIResponsesError::Upstream(format!("{} {}", status, body)));
    }
    Ok(response)
}

/// 构造请求头。
fn build_headers(
    model: &Model,
    options: &StreamOptions,
    auth: &Auth,
    api_key: &str,
) -> Result<HeaderMap, OpenAIResponsesError> {
    let mut headers = build_models_headers(options, auth, api_key)?;
    for (key, value) in &model.headers {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|error| OpenAIResponsesError::InvalidHeader(error.to_string()))?;
        let value =
            HeaderValue::from_str(value).map_err(|error| OpenAIResponsesError::InvalidHeader(error.to_string()))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

/// 构造 Models API 请求头。
fn build_models_headers(
    options: &StreamOptions,
    auth: &Auth,
    api_key: &str,
) -> Result<HeaderMap, OpenAIResponsesError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))
            .map_err(|error| OpenAIResponsesError::InvalidHeader(error.to_string()))?,
    );
    for (key, value) in options.headers.iter().chain(auth.headers.iter()) {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|error| OpenAIResponsesError::InvalidHeader(error.to_string()))?;
        let value =
            HeaderValue::from_str(value).map_err(|error| OpenAIResponsesError::InvalidHeader(error.to_string()))?;
        headers.insert(name, value);
    }
    if let Some(session_id) = &options.session_id {
        headers.insert(
            "x-client-request-id",
            HeaderValue::from_str(session_id)
                .map_err(|error| OpenAIResponsesError::InvalidHeader(error.to_string()))?,
        );
    }
    Ok(headers)
}

/// 解析 API Key，优先使用认证信息，其次使用环境变量。
fn resolve_api_key(model: &Model, auth: &Auth) -> Result<String, OpenAIResponsesError> {
    if let Some(api_key) = &auth.api_key {
        return Ok(api_key.clone());
    }
    let provider_key = format!("{}_API_KEY", model.provider.to_ascii_uppercase().replace('-', "_"));
    std::env::var(provider_key)
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| OpenAIResponsesError::MissingApiKey)
}

/// 将 OpenAI model 条目转换为内部 Model 元数据。
fn openai_model_from_value(value: &Value, base_url: &str, provider: &str) -> Option<Model> {
    let id = value.get("id").and_then(Value::as_str)?.to_string();
    Some(Model {
        id: id.clone(),
        name: id,
        api: OpenAIResponsesProvider::API.to_string(),
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
            .collect::<HashMap<_, _>>(),
    }
}

/// 处理 OpenAI SSE 响应。
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
        for event in process_sse_chunk(chunk.map_err(OpenAIResponsesError::from)?, &mut buffer)? {
            output = process_openai_event(event, model, output, sink, &mut state).await?;
        }
    }
    Ok(output)
}

/// 当前流处理状态。
#[derive(Default)]
struct StreamState {
    /// 当前内容块下标。
    current_index: Option<usize>,
    /// 当前工具调用参数的流式 JSON 暂存。
    partial_json: String,
}

/// 按 SSE 行协议切分 chunk。
fn process_sse_chunk(chunk: Bytes, buffer: &mut String) -> Result<Vec<Value>, OpenAIResponsesError> {
    let mut events = Vec::new();
    buffer.push_str(&String::from_utf8_lossy(&chunk));
    while let Some(position) = buffer.find('\n') {
        let line = buffer[..position].trim_end_matches('\r').to_string();
        buffer.drain(..=position);
        let Some(data) = line.trim().strip_prefix("data: ") else { continue };
        if data == "[DONE]" || data.is_empty() {
            continue;
        }
        events.push(serde_json::from_str(data)?);
    }
    Ok(events)
}

/// 处理单个 OpenAI 流事件。
async fn process_openai_event(
    event: Value,
    model: &Model,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or_default();
    match event_type {
        "response.created" => {
            output.response_id = event.pointer("/response/id").and_then(Value::as_str).map(ToOwned::to_owned);
        }
        "response.output_item.added" => output = handle_output_item_added(&event, output, sink, state).await?,
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            output = handle_thinking_delta(&event, output, sink, state).await?;
        }
        "response.output_text.delta" | "response.refusal.delta" => {
            output = handle_text_delta(&event, output, sink, state).await?;
        }
        "response.function_call_arguments.delta" => output = handle_tool_delta(&event, output, sink, state).await?,
        "response.function_call_arguments.done" => output = handle_tool_done(&event, output, sink, state).await?,
        "response.output_item.done" => output = handle_output_item_done(&event, output, sink, state).await?,
        "response.completed" => handle_response_completed(&event, model, &mut output),
        "response.failed" => {
            return Err(OpenAIResponsesError::Upstream(openai_responses_failure_message(&event)).into())
        }
        "error" => return Err(OpenAIResponsesError::Upstream(openai_responses_error_message(&event)).into()),
        _ => {}
    }
    Ok(output)
}

/// 处理 output item 开始事件。
async fn handle_output_item_added(
    event: &Value,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    let item = &event["item"];
    match item.get("type").and_then(Value::as_str) {
        Some("message") => {
            output.content.push(ContentBlock::Text(TextContent { text: String::new(), text_signature: None }));
            let index = output.content.len() - 1;
            state.current_index = Some(index);
            output = sink.emit(AssistantMessageEvent::TextStart { content_index: index, partial: output }).await?;
        }
        Some("function_call") => {
            let id = format!(
                "{}|{}",
                item.get("call_id").and_then(Value::as_str).unwrap_or_default(),
                item.get("id").and_then(Value::as_str).unwrap_or_default()
            );
            output.content.push(ContentBlock::ToolCall(ToolCall {
                id,
                name: item.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                arguments: Map::new(),
                thought_signature: None,
            }));
            let index = output.content.len() - 1;
            state.current_index = Some(index);
            state.partial_json = item.get("arguments").and_then(Value::as_str).unwrap_or_default().to_string();
            output = sink.emit(AssistantMessageEvent::ToolCallStart { content_index: index, partial: output }).await?;
        }
        Some("reasoning") => {
            output.content.push(ContentBlock::Thinking(ThinkingContent {
                thinking: String::new(),
                thinking_signature: None,
                redacted: None,
            }));
            let index = output.content.len() - 1;
            state.current_index = Some(index);
            output = sink.emit(AssistantMessageEvent::ThinkingStart { content_index: index, partial: output }).await?;
        }
        _ => {}
    }
    Ok(output)
}

/// 处理文本增量事件。
async fn handle_text_delta(
    event: &Value,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &StreamState,
) -> Result<AssistantMessage, StreamError> {
    let Some(index) = state.current_index else { return Ok(output) };
    let delta = event.get("delta").and_then(Value::as_str).unwrap_or_default().to_string();
    let Some(ContentBlock::Text(text)) = output.content.get_mut(index) else { return Ok(output) };
    text.text.push_str(&delta);
    sink.emit(AssistantMessageEvent::TextDelta { content_index: index, delta, partial: output }).await
}

/// 处理 reasoning 摘要或推理文本增量。
async fn handle_thinking_delta(
    event: &Value,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &StreamState,
) -> Result<AssistantMessage, StreamError> {
    let Some(index) = state.current_index else { return Ok(output) };
    let delta = event.get("delta").and_then(Value::as_str).unwrap_or_default().to_string();
    let Some(ContentBlock::Thinking(thinking)) = output.content.get_mut(index) else { return Ok(output) };
    thinking.thinking.push_str(&delta);
    sink.emit(AssistantMessageEvent::ThinkingDelta { content_index: index, delta, partial: output }).await
}

/// 处理工具参数增量事件。
async fn handle_tool_delta(
    event: &Value,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    let Some(index) = state.current_index else { return Ok(output) };
    let delta = event.get("delta").and_then(Value::as_str).unwrap_or_default().to_string();
    state.partial_json.push_str(&delta);
    let Some(ContentBlock::ToolCall(tool_call)) = output.content.get_mut(index) else { return Ok(output) };
    if let Value::Object(arguments) = parse_streaming_json_value(Some(&state.partial_json)) {
        tool_call.arguments = arguments;
    }
    sink.emit(AssistantMessageEvent::ToolCallDelta { content_index: index, delta, partial: output }).await
}

/// 处理工具参数完成事件。
async fn handle_tool_done(
    event: &Value,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    let Some(index) = state.current_index else { return Ok(output) };
    let arguments = event.get("arguments").and_then(Value::as_str).unwrap_or_default();
    state.partial_json = arguments.to_string();
    let Some(ContentBlock::ToolCall(tool_call)) = output.content.get_mut(index) else { return Ok(output) };
    if let Value::Object(arguments) = parse_streaming_json_value(Some(&state.partial_json)) {
        tool_call.arguments = arguments;
    }
    sink.emit(AssistantMessageEvent::ToolCallDelta { content_index: index, delta: String::new(), partial: output })
        .await
}

/// 处理 output item 完成事件。
async fn handle_output_item_done(
    event: &Value,
    mut output: AssistantMessage,
    sink: &mut dyn AssistantMessageEventSink,
    state: &mut StreamState,
) -> Result<AssistantMessage, StreamError> {
    if let Some(image) = image_from_output_item(&event["item"]) {
        output.content.push(ContentBlock::Image(image));
        return Ok(output);
    }
    let Some(index) = state.current_index else { return Ok(output) };
    match output.content.get(index) {
        Some(ContentBlock::Text(_)) => {
            output = sink.emit(AssistantMessageEvent::TextEnd { content_index: index, partial: output }).await?;
        }
        Some(ContentBlock::Thinking(_)) => {
            output = sink.emit(AssistantMessageEvent::ThinkingEnd { content_index: index, partial: output }).await?;
        }
        Some(ContentBlock::ToolCall(_)) => {
            output = sink.emit(AssistantMessageEvent::ToolCallEnd { content_index: index, partial: output }).await?;
        }
        Some(ContentBlock::Image(_)) => {}
        None => {}
    }
    state.current_index = None;
    state.partial_json.clear();
    Ok(output)
}

/// 从已完成的 OpenAI 图片生成项中提取 Base64 图像数据。
fn image_from_output_item(item: &Value) -> Option<ImageContent> {
    if item.get("type").and_then(Value::as_str) != Some("image_generation_call") {
        return None;
    }
    let data = item.get("result").and_then(Value::as_str)?;
    let (mime_type, data) = data
        .strip_prefix("data:")
        .and_then(|value| value.split_once(";base64,"))
        .map_or(("image/png", data), |(mime_type, data)| (mime_type, data));
    Some(ImageContent { data: data.to_string(), mime_type: mime_type.to_string() })
}

/// 处理最终 completed 事件。
fn handle_response_completed(event: &Value, model: &Model, output: &mut AssistantMessage) {
    let response = &event["response"];
    if let Some(id) = response.get("id").and_then(Value::as_str) {
        output.response_id = Some(id.to_string());
    }
    if let Some(usage) = response.get("usage") {
        let input_tokens = usage.get("input_tokens").and_then(Value::as_u64).unwrap_or_default();
        let output_tokens = usage.get("output_tokens").and_then(Value::as_u64).unwrap_or_default();
        let cached_tokens =
            usage.pointer("/input_tokens_details/cached_tokens").and_then(Value::as_u64).unwrap_or_default();
        output.usage.input = input_tokens.saturating_sub(cached_tokens);
        output.usage.output = output_tokens;
        output.usage.cache_read = cached_tokens;
        output.usage.cache_write = 0;
        output.usage.total_tokens =
            usage.get("total_tokens").and_then(Value::as_u64).unwrap_or(input_tokens + output_tokens);
        calculate_cost(model, &mut output.usage);
    }
    output.stop_reason = match response.get("status").and_then(Value::as_str) {
        Some("incomplete") => StopReason::Length,
        Some("failed") | Some("cancelled") => StopReason::Error,
        _ => StopReason::Stop,
    };
}

/// 计算用量成本。
fn calculate_cost(model: &Model, usage: &mut Usage) {
    usage.cost.input = model.cost.input / 1_000_000.0 * usage.input as f64;
    usage.cost.output = model.cost.output / 1_000_000.0 * usage.output as f64;
    usage.cost.cache_read = model.cost.cache_read / 1_000_000.0 * usage.cache_read as f64;
    usage.cost.cache_write = model.cost.cache_write / 1_000_000.0 * usage.cache_write as f64;
    usage.cost.total = usage.cost.input + usage.cost.output + usage.cost.cache_read + usage.cost.cache_write;
}

/// 构造上游 failed 错误消息。
fn openai_responses_failure_message(event: &Value) -> String {
    event.pointer("/response/error/message").and_then(Value::as_str).unwrap_or("response.failed").to_string()
}

/// 构造上游 error 错误消息。
fn openai_responses_error_message(event: &Value) -> String {
    event.get("message").and_then(Value::as_str).unwrap_or("error").to_string()
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
        timestamp: now_millis(),
    }
}
