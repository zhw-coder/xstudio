//! OpenAI Images API 图像 Provider。
//! 本实现使用非流式 `POST /images/generations` 与 `/images/edits` 协议，不依赖 OpenAI SDK。

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    multipart::{Form, Part},
};
use serde_json::{json, Map, Value};
use std::{collections::HashMap, time::Duration};

use crate::model::{
    api_registry::{ApiProvider, AssistantMessageEventSink},
    stream::{apply_provider_payload_callback, notify_provider_response_callback},
    types::*,
};

/// OpenAI Images 图像 Provider。
#[derive(Debug, Default, Clone)]
pub struct OpenAIImagesProvider;

impl OpenAIImagesProvider {
    /// OpenAI Images API 标识。
    pub const API: &'static str = "OpenAI-Images";
}

#[async_trait]
impl ApiProvider for OpenAIImagesProvider {
    /// 调用 OpenAI Models API 获取可用模型列表。
    async fn models(
        &self,
        provider: &str,
        base_url: &str,
        options: &StreamOptions,
        auth: &Auth,
    ) -> Result<Vec<Model>, StreamError> {
        list_openai_images_models(provider, base_url, options, auth).await
    }

    /// 以既有事件协议调用非流式图像生成 API。
    async fn stream(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        stream_openai_images(model, context, options, auth, sink).await
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
        stream_openai_images(model, context, options, auth, sink).await
    }
}

/// 执行一次 OpenAI Images 请求。
///
/// # 参数
/// - `model`：图像生成模型。
/// - `context`：生成提示上下文。
/// - `options`：请求选项。
/// - `auth`：认证信息。
pub async fn stream_openai_images(
    model: &Model,
    context: Context,
    options: &StreamOptions,
    auth: &Auth,
    sink: &mut dyn AssistantMessageEventSink,
) -> Result<AssistantMessage, StreamError> {
    let api_key = resolve_api_key(model, auth)?;
    let request = build_image_request(context)?;
    let payload =
        apply_provider_payload_callback(options, build_payload(model, &request.prompt, options)?, model).await?;
    let client = reqwest::Client::builder()
        .timeout(options.timeout_ms.map(Duration::from_millis).unwrap_or(Duration::from_secs(600)))
        .build()
        .map_err(OpenAIImagesError::from)?;
    let response = if request.images.is_empty() {
        client
            .post(format!("{}/images/generations", model.base_url.trim_end_matches('/')))
            .headers(build_headers(model, options, auth, &api_key)?)
            .json(&payload)
            .send()
            .await
    } else {
        let mut headers = build_headers(model, options, auth, &api_key)?;
        // 由 multipart 自动填充带 boundary 的 Content-Type。
        headers.remove(CONTENT_TYPE);
        client
            .post(format!("{}/images/edits", model.base_url.trim_end_matches('/')))
            .headers(headers)
            .multipart(build_edit_form(payload, request.images)?)
            .send()
            .await
    }
    .map_err(OpenAIImagesError::from)?;
    notify_provider_response_callback(options, provider_response(&response), model).await?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(OpenAIImagesError::Upstream(format!("{status} {body}")).into());
    }
    let body: Value = response.json().await.map_err(OpenAIImagesError::from)?;
    let partial = sink.emit(AssistantMessageEvent::Start { partial: empty_partial_message(model) }).await?;
    if partial.stop_reason == StopReason::Aborted {
        return Ok(partial);
    }
    let output = response_from_value(model, &body)?;
    sink.emit(AssistantMessageEvent::Done { reason: output.stop_reason, message: output }).await
}

/// 调用 OpenAI `GET /models` 并转换为内部 Model 元数据。
async fn list_openai_images_models(
    provider: &str,
    base_url: &str,
    options: &StreamOptions,
    auth: &Auth,
) -> Result<Vec<Model>, StreamError> {
    let api_key = auth.api_key.as_deref().ok_or(OpenAIImagesError::MissingApiKey)?;
    let client = reqwest::Client::builder()
        .timeout(options.timeout_ms.map(Duration::from_millis).unwrap_or(Duration::from_secs(60)))
        .build()
        .map_err(OpenAIImagesError::from)?;
    let response = client
        .get(format!("{}/models", base_url.trim_end_matches('/')))
        .headers(build_headers(&Model::default(), options, auth, api_key)?)
        .send()
        .await
        .map_err(OpenAIImagesError::from)?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(OpenAIImagesError::Upstream(format!("{status} {body}")).into());
    }
    let body: Value = response.json().await.map_err(OpenAIImagesError::from)?;
    let mut models = body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .map(|id| Model {
                    id: id.to_string(),
                    name: id.to_string(),
                    api: OpenAIImagesProvider::API.to_string(),
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
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(models)
}

/// OpenAI Images Provider 内部错误。
#[derive(Debug, thiserror::Error)]
enum OpenAIImagesError {
    /// 缺失 API Key。
    #[error("OpenAI API key is required. Set OPENAI_API_KEY or pass auth.api_key")]
    MissingApiKey,
    /// HTTP 层错误。
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    /// Header 构造错误。
    #[error("Invalid header: {0}")]
    InvalidHeader(String),
    /// Base64 图像数据无效。
    #[error("Invalid image data: {0}")]
    InvalidImageData(String),
    /// 图像 MIME 类型无效。
    #[error("Invalid image MIME type: {0}")]
    InvalidImageMimeType(String),
    /// 响应格式错误。
    #[error("Invalid OpenAI Images response: {0}")]
    InvalidResponse(String),
    /// 上游返回错误。
    #[error("OpenAI error: {0}")]
    Upstream(String),
}

impl From<OpenAIImagesError> for StreamError {
    /// 将 Provider 内部错误转换为通用流错误。
    fn from(error: OpenAIImagesError) -> Self {
        Self::Stream(error.to_string())
    }
}

/// 构造 OpenAI Images 请求 payload。
fn build_payload(model: &Model, prompt: &str, options: &StreamOptions) -> Result<Value, OpenAIImagesError> {
    if prompt.trim().is_empty() {
        return Err(OpenAIImagesError::InvalidResponse("a text prompt is required".to_string()));
    }
    let mut payload = Map::new();
    payload.insert("model".into(), json!(model.id));
    payload.insert("prompt".into(), json!(prompt));
    payload.insert("response_format".into(), json!("b64_json"));
    for (key, value) in &options.metadata {
        payload.insert(key.clone(), value.clone());
    }
    Ok(Value::Object(payload))
}

/// 构造 OpenAI Images Edits multipart 请求体。
fn build_edit_form(payload: Value, images: Vec<ImageContent>) -> Result<Form, OpenAIImagesError> {
    let fields = payload
        .as_object()
        .ok_or_else(|| OpenAIImagesError::InvalidResponse("payload must be a JSON object".to_string()))?;
    let mut form = Form::new();
    for (key, value) in fields {
        let value = match value {
            Value::String(value) => value.clone(),
            value => value.to_string(),
        };
        form = form.text(key.clone(), value);
    }
    for (index, image) in images.into_iter().enumerate() {
        let data =
            STANDARD.decode(image.data).map_err(|error| OpenAIImagesError::InvalidImageData(error.to_string()))?;
        let part = Part::bytes(data)
            .file_name(format!("image-{index}"))
            .mime_str(&image.mime_type)
            .map_err(|error| OpenAIImagesError::InvalidImageMimeType(error.to_string()))?;
        form = form.part("image", part);
    }
    Ok(form)
}

/// 构造 OpenAI Images 请求头。
fn build_headers(
    model: &Model,
    options: &StreamOptions,
    auth: &Auth,
    api_key: &str,
) -> Result<HeaderMap, OpenAIImagesError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|error| OpenAIImagesError::InvalidHeader(error.to_string()))?,
    );
    for (key, value) in options.headers.iter().chain(auth.headers.iter()).chain(model.headers.iter()) {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|error| OpenAIImagesError::InvalidHeader(error.to_string()))?;
        let value =
            HeaderValue::from_str(value).map_err(|error| OpenAIImagesError::InvalidHeader(error.to_string()))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

/// 解析 API Key，优先使用认证信息，其次使用环境变量。
fn resolve_api_key(model: &Model, auth: &Auth) -> Result<String, OpenAIImagesError> {
    if let Some(api_key) = &auth.api_key {
        return Ok(api_key.clone());
    }
    let provider_key = format!("{}_API_KEY", model.provider.to_ascii_uppercase().replace('-', "_"));
    std::env::var(provider_key)
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .map_err(|_| OpenAIImagesError::MissingApiKey)
}

/// 将 OpenAI Images 响应转换为内部结果。
fn response_from_value(model: &Model, value: &Value) -> Result<AssistantMessage, StreamError> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| OpenAIImagesError::InvalidResponse("missing data array".to_string()))?;
    let output = data
        .iter()
        .filter_map(|image| image.get("b64_json").and_then(Value::as_str))
        .map(|data| ContentBlock::Image(ImageContent { data: data.to_string(), mime_type: "image/png".to_string() }))
        .collect();
    Ok(AssistantMessage {
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        content: output,
        response_id: value.get("id").and_then(Value::as_str).map(str::to_string),
        response_model: None,
        diagnostics: Vec::new(),
        usage: empty_usage(),
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: now_millis(),
    })
}

/// OpenAI Images 请求计划。
struct ImageRequest {
    /// 拼接后的文本提示词。
    prompt: String,
    /// 需要上传的参考图片。
    images: Vec<ImageContent>,
}

/// 从上下文构造 Images 请求计划。
///
/// 不使用系统提示词和 ToolResult；仅保留最后一条 User 的全部文本、图片，以及最后一条
/// Assistant 中最后出现的文本或图片块。
fn build_image_request(context: Context) -> Result<ImageRequest, OpenAIImagesError> {
    let mut last_user = None;
    let mut last_assistant = None;
    for message in context.messages.into_iter().rev() {
        match message {
            Message::User(message) if last_user.is_none() => last_user = Some(message),
            Message::Assistant(message) if last_assistant.is_none() => last_assistant = Some(message),
            Message::User(_) | Message::Assistant(_) | Message::ToolResult(_) => {}
        }
        if last_user.is_some() && last_assistant.is_some() {
            break;
        }
    }
    let mut parts = Vec::new();
    let mut images = Vec::new();

    if let Some(message) = last_user {
        match message.content {
            UserContent::Text(text) => parts.push(text),
            UserContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::Text(text) => parts.push(text.text),
                        ContentBlock::Image(image) => images.push(image),
                        ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) => {}
                    }
                }
            }
        }
    }
    if let Some(message) = last_assistant {
        if let Some(block) = message
            .content
            .into_iter()
            .rev()
            .find(|block| matches!(block, ContentBlock::Text(_) | ContentBlock::Image(_)))
        {
            match block {
                ContentBlock::Text(text) => parts.push(text.text),
                ContentBlock::Image(image) => images.push(image),
                ContentBlock::Thinking(_) | ContentBlock::ToolCall(_) => unreachable!(),
            }
        }
    }
    Ok(ImageRequest { prompt: parts.join("\n"), images })
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

/// 构造客户端维护的空 partial AssistantMessage。
fn empty_partial_message(model: &Model) -> AssistantMessage {
    AssistantMessage {
        content: Vec::new(),
        api: model.api.clone(),
        provider: model.provider.clone(),
        model: model.id.clone(),
        response_model: None,
        response_id: None,
        diagnostics: Vec::new(),
        usage: empty_usage(),
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: now_millis(),
    }
}
