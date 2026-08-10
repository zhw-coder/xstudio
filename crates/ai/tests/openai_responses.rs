//! OpenAI Responses SSE 集成测试。

use ai::model::{
    providers::texts::openai_responses::stream_openai_responses, AssistantMessage, AssistantMessageEvent,
    AssistantMessageEventSink, Auth, ContentBlock, Context, Model, StreamError, StreamOptions, ThinkingContent,
};
use async_trait::async_trait;
use tokio::{io::AsyncWriteExt, net::TcpListener};

/// 收集 Provider 事件，并返回其中的 partial 消息。
#[derive(Default)]
struct CollectingSink {
    events: Vec<AssistantMessageEvent>,
}

#[async_trait]
impl AssistantMessageEventSink for CollectingSink {
    /// 保存事件并返回对应 partial 消息。
    async fn emit(&mut self, event: AssistantMessageEvent) -> Result<AssistantMessage, StreamError> {
        let partial = match &event {
            AssistantMessageEvent::Start { partial }
            | AssistantMessageEvent::TextStart { partial, .. }
            | AssistantMessageEvent::TextDelta { partial, .. }
            | AssistantMessageEvent::TextEnd { partial, .. }
            | AssistantMessageEvent::ThinkingStart { partial, .. }
            | AssistantMessageEvent::ThinkingDelta { partial, .. }
            | AssistantMessageEvent::ThinkingEnd { partial, .. }
            | AssistantMessageEvent::ToolCallStart { partial, .. }
            | AssistantMessageEvent::ToolCallDelta { partial, .. }
            | AssistantMessageEvent::ToolCallEnd { partial, .. } => partial.clone(),
            AssistantMessageEvent::Done { message, .. } => message.clone(),
        };
        self.events.push(event);
        Ok(partial)
    }
}

/// 启动仅响应一次的本地 SSE 服务。
async fn start_sse_server(events: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener should bind");
    let address = listener.local_addr().expect("listener should have an address");
    let events = events.to_string();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("server should accept request");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{events}",
            events.len()
        );
        stream.write_all(response.as_bytes()).await.expect("server should write response");
    });
    format!("http://{address}")
}

/// 构造本地 SSE Provider 使用的测试模型。
fn test_model(base_url: String) -> Model {
    Model { id: "gpt-test".to_string(), base_url, ..Model::default() }
}

/// 验证 reasoning summary 与 reasoning text delta 会按顺序转换为 Thinking 事件。
#[tokio::test]
async fn stream_accumulates_reasoning_deltas() {
    let base_url = start_sse_server(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"reasoning\"}}\n\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"摘要\"}\n\ndata: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"推理\"}\n\ndata: {\"type\":\"response.output_item.done\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-1\",\"status\":\"completed\"}}\n\ndata: [DONE]\n\n",
    )
    .await;
    let mut sink = CollectingSink::default();
    let output = stream_openai_responses(
        &test_model(base_url),
        Context::default(),
        &StreamOptions::default(),
        &Auth { api_key: Some("test-key".to_string()), ..Default::default() },
        &mut sink,
    )
    .await
    .expect("local SSE stream should succeed");
    let deltas = sink
        .events
        .iter()
        .filter_map(|event| match event {
            AssistantMessageEvent::ThinkingDelta { content_index, delta, .. } => Some((*content_index, delta.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(deltas, vec![(0, "摘要"), (0, "推理")]);
    assert!(matches!(sink.events.first(), Some(AssistantMessageEvent::Start { .. })));
    assert!(sink.events.iter().any(|event| matches!(event, AssistantMessageEvent::ThinkingStart { .. })));
    assert!(sink.events.iter().any(|event| matches!(event, AssistantMessageEvent::ThinkingEnd { .. })));
    assert!(matches!(sink.events.last(), Some(AssistantMessageEvent::Done { .. })));
    assert!(matches!(
        output.content.as_slice(),
        [ContentBlock::Thinking(ThinkingContent { thinking, .. })] if thinking == "摘要推理"
    ));
}

/// 验证图片生成完成项会转换为最终消息中的图片内容块。
#[tokio::test]
async fn stream_collects_generated_image() {
    let base_url = start_sse_server(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"image_generation_call\",\"result\":\"aW1hZ2U=\"}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"response-image\",\"status\":\"completed\"}}\n\ndata: [DONE]\n\n",
    )
    .await;
    let mut sink = CollectingSink::default();
    let output = stream_openai_responses(
        &test_model(base_url),
        Context::default(),
        &StreamOptions::default(),
        &Auth { api_key: Some("test-key".to_string()), ..Default::default() },
        &mut sink,
    )
    .await
    .expect("local SSE stream should succeed");

    assert!(matches!(
        output.content.as_slice(),
        [ContentBlock::Image(image)] if image.data == "aW1hZ2U=" && image.mime_type == "image/png"
    ));
}
