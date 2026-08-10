//! OpenAI Images Provider 集成测试。

use ai::model::{
    providers::images::openai_images::{stream_openai_images, OpenAIImagesProvider},
    AssistantMessage, AssistantMessageEvent, AssistantMessageEventSink, Auth, ContentBlock, Context, ImageContent,
    Message, Model, StreamError, StreamOptions, ToolResultMessage, UserContent, UserMessage,
};
use async_trait::async_trait;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// 收集 Provider 事件，并返回事件中的消息。
#[derive(Default)]
struct CollectingSink {
    events: Vec<AssistantMessageEvent>,
}

#[async_trait]
impl AssistantMessageEventSink for CollectingSink {
    /// 保存事件并返回对应消息。
    async fn emit(&mut self, event: AssistantMessageEvent) -> Result<AssistantMessage, StreamError> {
        let message = match &event {
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
        Ok(message)
    }
}

/// 启动只接受一次请求的本地 Images 服务。
async fn start_images_server() -> (String, tokio::sync::oneshot::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener should bind");
    let address = listener.local_addr().expect("listener should have an address");
    let (payload_sender, payload_receiver) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("server should accept request");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stream.read(&mut buffer).await.expect("server should read request");
            assert_ne!(count, 0, "client should send a complete request");
            request.extend_from_slice(&buffer[..count]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&request[..header_end]).expect("headers should be UTF-8");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())
                })
                .flatten()
                .expect("request should contain content length");
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        payload_sender.send(request).expect("request receiver should wait");
        let body = r#"{"created":1,"data":[{"b64_json":"image-data"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.expect("server should write response");
    });
    (format!("http://{address}"), payload_receiver)
}

/// 从完整 HTTP 请求中提取请求体。
fn request_body(request: &[u8]) -> &[u8] {
    let header_end =
        request.windows(4).position(|window| window == b"\r\n\r\n").expect("request should contain headers");
    &request[header_end + 4..]
}

/// 构造 OpenAI Images 测试模型。
fn test_model(base_url: String) -> Model {
    Model {
        id: "gpt-image-1".to_string(),
        api: OpenAIImagesProvider::API.to_string(),
        base_url,
        ..Default::default()
    }
}

/// 构造 Base64 图像块。
fn image(data: &str) -> ContentBlock {
    ContentBlock::Image(ImageContent { data: data.to_string(), mime_type: "image/png".to_string() })
}

/// 以指定上下文调用本地 Images 服务。
async fn request_images(context: Context) -> (AssistantMessage, CollectingSink, Vec<u8>) {
    let (base_url, request_receiver) = start_images_server().await;
    let mut sink = CollectingSink::default();
    let output = stream_openai_images(
        &test_model(base_url),
        context,
        &StreamOptions::default(),
        &Auth { api_key: Some("test-key".to_string()), ..Default::default() },
        &mut sink,
    )
    .await
    .expect("local Images request should succeed");
    (output, sink, request_receiver.await.expect("server should receive request"))
}

/// 验证无图片时使用 Generations JSON 请求，并将响应转换为内部图片块。
#[tokio::test]
async fn stream_uses_generations_for_text_only_context() {
    let context = Context {
        messages: vec![Message::User(UserMessage {
            content: UserContent::Text("a cat".to_string()),
            timestamp: 0,
        })],
        ..Default::default()
    };

    let (output, sink, request) = request_images(context).await;
    let request = std::str::from_utf8(&request).expect("request should be UTF-8");
    let payload: serde_json::Value =
        serde_json::from_slice(request_body(request.as_bytes())).expect("request should contain JSON");

    assert!(request.starts_with("POST /images/generations HTTP/1.1"));
    assert_eq!(payload["response_format"], "b64_json");
    assert_eq!(payload["prompt"], "a cat");
    assert!(matches!(sink.events.first(), Some(AssistantMessageEvent::Start { .. })));
    assert!(matches!(sink.events.last(), Some(AssistantMessageEvent::Done { .. })));
    assert!(matches!(output.content.as_slice(), [ContentBlock::Image(image)] if image.data == "image-data"));
}

/// 验证编辑请求仅使用最后一条 User 的全部内容和最后一条 Assistant 的最后有效内容。
#[tokio::test]
async fn stream_uses_edits_with_selected_context_images() {
    let context = Context {
        system_prompt: Some("system prompt".to_string()),
        messages: vec![
            Message::User(UserMessage {
                content: UserContent::Blocks(vec![
                    ContentBlock::Text(ai::model::TextContent {
                        text: "earlier user".to_string(),
                        text_signature: None,
                    }),
                    image("dXNlci0x"),
                ]),
                timestamp: 0,
            }),
            Message::Assistant(AssistantMessage { content: vec![image("YXNzaXN0YW50LWVhcmx5")], ..Default::default() }),
            Message::User(UserMessage {
                content: UserContent::Blocks(vec![
                    ContentBlock::Text(ai::model::TextContent { text: "last user".to_string(), text_signature: None }),
                    image("dXNlci0y"),
                    image("dXNlci0z"),
                ]),
                timestamp: 0,
            }),
            Message::Assistant(AssistantMessage {
                content: vec![
                    ContentBlock::Text(ai::model::TextContent {
                        text: "last assistant text".to_string(),
                        text_signature: None,
                    }),
                    image("YXNzaXN0YW50LWxhc3Q="),
                ],
                ..Default::default()
            }),
            Message::ToolResult(ToolResultMessage {
                tool_call_id: "tool-1".to_string(),
                tool_name: "image_tool".to_string(),
                content: vec![
                    ContentBlock::Text(ai::model::TextContent { text: "tool text".to_string(), text_signature: None }),
                    image("dG9vbC1pbWFnZQ=="),
                ],
                details: None,
                is_error: false,
                timestamp: 0,
            }),
        ],
        ..Default::default()
    };

    let (_, _, request) = request_images(context).await;
    let request = std::str::from_utf8(&request).expect("multipart request should be UTF-8");

    assert!(request.starts_with("POST /images/edits HTTP/1.1"));
    assert!(request.contains("content-type: multipart/form-data; boundary="));
    assert_eq!(request.matches("name=\"image\"").count(), 3);
    assert!(request.contains("last user"));
    assert!(request.contains("assistant-last"));
    assert!(!request.contains("system prompt"));
    assert!(!request.contains("earlier user"));
    assert!(!request.contains("user-1"));
    assert!(!request.contains("assistant-early"));
    assert!(!request.contains("last assistant text"));
    assert!(!request.contains("tool-image"));
    assert!(!request.contains("tool text"));
    assert!(
        request.find("user-2").expect("first user image should exist")
            < request.find("user-3").expect("second user image should exist")
    );
    assert!(
        request.find("user-3").expect("second user image should exist")
            < request.find("assistant-last").expect("last assistant image should exist")
    );
}
