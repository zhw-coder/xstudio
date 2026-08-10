//! OpenAI Chat Completions SSE 集成测试。

use ai::model::{
    providers::texts::openai_completions::stream_openai_completions, AssistantMessage, AssistantMessageEvent,
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

/// 验证分片 SSE、工具参数与 reasoning 扩展会转换为内部流事件。
#[tokio::test]
async fn stream_accumulates_tool_arguments_and_reasoning() {
    let base_url = start_sse_server(
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"分析\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"结果\",\"reasoning_details\":[{\"type\":\"reasoning.encrypted\",\"data\":\"opaque\"}]},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"src\"}}]},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"/lib.rs\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
    )
    .await;
    let mut sink = CollectingSink::default();
    let output = stream_openai_completions(
        &test_model(base_url),
        Context::default(),
        &StreamOptions::default(),
        &Auth { api_key: Some("test-key".to_string()), ..Default::default() },
        &mut sink,
    )
    .await
    .expect("local SSE stream should succeed");

    assert!(matches!(sink.events.first(), Some(AssistantMessageEvent::Start { .. })));
    assert!(sink.events.iter().any(|event| matches!(event, AssistantMessageEvent::ThinkingStart { .. })));
    assert!(sink.events.iter().any(|event| matches!(event, AssistantMessageEvent::ThinkingEnd { .. })));
    assert!(matches!(sink.events.last(), Some(AssistantMessageEvent::Done { .. })));
    assert!(matches!(
        output.content.as_slice(),
        [
            ContentBlock::Thinking(ThinkingContent {
                thinking,
                thinking_signature: Some(signature),
                ..
            }),
            ContentBlock::ToolCall(tool_call),
        ] if thinking == "分析结果"
            && signature == "[{\"data\":\"opaque\",\"type\":\"reasoning.encrypted\"}]"
            && tool_call.arguments["path"] == "src/lib.rs"
    ));
}
