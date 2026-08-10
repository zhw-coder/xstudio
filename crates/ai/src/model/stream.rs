//! 文本类模型流式 / 非流式调用入口。具体 Provider 通过 `ApiRegistry` 注入。

use crate::model::{
    api_registry::{ApiRegistry, AssistantMessageEventSink},
    types::*,
};

/// 以回调事件形式调用文本/聊天模型。
pub async fn stream(
    model: &Model,
    context: Context,
    options: &StreamOptions,
    auth: &Auth,
    sink: &mut dyn AssistantMessageEventSink,
) -> Result<AssistantMessage, StreamError> {
    let provider =
        ApiRegistry::global().get(&model.api).ok_or_else(|| StreamError::NoApiProvider(model.api.clone()))?;
    provider.stream(model, context, options, auth, sink).await
}

/// 以“等待最终结果”的方式调用文本/聊天模型。
pub async fn complete(
    model: &Model,
    context: Context,
    options: &StreamOptions,
    auth: &Auth,
) -> Result<AssistantMessage, StreamError> {
    let mut sink = FinalMessageSink::default();
    stream(model, context, options, auth, &mut sink).await
}

/// 简化版回调事件入口。
pub async fn stream_simple(
    model: &Model,
    context: Context,
    options: &StreamOptions,
    auth: &Auth,
    sink: &mut dyn AssistantMessageEventSink,
) -> Result<AssistantMessage, StreamError> {
    let provider =
        ApiRegistry::global().get(&model.api).ok_or_else(|| StreamError::NoApiProvider(model.api.clone()))?;
    provider.stream_simple(model, context, options, auth, sink).await
}

/// 简化版“等待最终结果”入口。
pub async fn complete_simple(
    model: &Model,
    context: Context,
    options: &StreamOptions,
    auth: &Auth,
) -> Result<AssistantMessage, StreamError> {
    let mut sink = FinalMessageSink::default();
    stream_simple(model, context, options, auth, &mut sink).await
}

/// 捕获最终 AssistantMessage 的内部 sink。
#[derive(Default)]
struct FinalMessageSink;

#[async_trait::async_trait]
impl AssistantMessageEventSink for FinalMessageSink {
    /// 保存最终事件中的消息快照。
    async fn emit(&mut self, event: AssistantMessageEvent) -> Result<AssistantMessage, StreamError> {
        Ok(match event {
            AssistantMessageEvent::Done { message, .. } => message,
            AssistantMessageEvent::Start { partial }
            | AssistantMessageEvent::TextStart { partial, .. }
            | AssistantMessageEvent::TextDelta { partial, .. }
            | AssistantMessageEvent::TextEnd { partial, .. }
            | AssistantMessageEvent::ThinkingStart { partial, .. }
            | AssistantMessageEvent::ThinkingDelta { partial, .. }
            | AssistantMessageEvent::ThinkingEnd { partial, .. }
            | AssistantMessageEvent::ToolCallStart { partial, .. }
            | AssistantMessageEvent::ToolCallDelta { partial, .. }
            | AssistantMessageEvent::ToolCallEnd { partial, .. } => partial,
        })
    }
}

/// 执行 Provider payload 回调，供具体 Provider 在发送 HTTP 请求前调用。
pub async fn apply_provider_payload_callback(
    options: &StreamOptions,
    payload: serde_json::Value,
    model: &Model,
) -> Result<serde_json::Value, StreamError> {
    if let Some(callback) = &options.on_payload {
        return callback.on_payload(payload, model).await;
    }
    Ok(payload)
}

/// 执行 Provider response 回调，供具体 Provider 在收到 HTTP 响应后调用。
pub async fn notify_provider_response_callback(
    options: &StreamOptions,
    response: ProviderResponse,
    model: &Model,
) -> Result<ProviderResponse, StreamError> {
    if let Some(callback) = &options.on_response {
        return callback.on_response(response, model).await;
    }
    Ok(response)
}
