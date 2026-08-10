//! Agent lifecycle 和 proxy cancellation 的集成测试。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use ai::{
    agent::{
        env::{LocalExecutionEnv, LocalExecutionEnvOptions},
        stream_proxy, Agent, AgentError, AgentEvent, AgentEventListener, AgentMessage, AgentOptions,
        ProxySerializableStreamOptions, ProxyStreamOptions, StreamFn,
    },
    model::{
        empty_usage, now_millis, AssistantMessage, AssistantMessageEvent, AssistantMessageEventSink, Auth,
        ContentBlock, Context, Model, StopReason, StreamError, StreamOptions, TextContent, UserContent, UserMessage,
    },
};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

/// 固定返回 assistant 响应的测试 stream 实现。
struct FixedStream;

#[async_trait]
impl StreamFn for FixedStream {
    /// 发送 start / done 事件并返回固定 assistant 消息。
    async fn stream<'a>(
        &'a self,
        model: &'a Model,
        _context: Context,
        _options: &'a StreamOptions,
        _auth: &'a Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, AgentError> {
        let message = test_assistant_message(model, "ok");
        sink.emit(AssistantMessageEvent::Start { partial: message.clone() }).await?;
        let message = sink.emit(AssistantMessageEvent::Done { reason: StopReason::Stop, message }).await?;
        Ok(message)
    }
}

/// 构造测试使用的模型元数据。
fn test_model() -> Model {
    Model {
        id: "test-model".to_string(),
        name: "Test Model".to_string(),
        api: "test-api".to_string(),
        provider: "test-provider".to_string(),
        base_url: "http://localhost".to_string(),
        ..Model::default()
    }
}

/// 构造测试使用的 assistant 消息。
fn test_assistant_message(model: &Model, text: &str) -> AssistantMessage {
    AssistantMessage {
        content: vec![ContentBlock::Text(TextContent { text: text.to_string(), text_signature: None })],
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

/// 构造测试使用的 user 消息。
fn test_user_message(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage { content: UserContent::Text(text.to_string()), timestamp: now_millis() })
}

/// 返回固定 assistant 响应的 fake stream。
fn fixed_stream() -> Box<dyn StreamFn> {
    Box::new(FixedStream)
}

/// 创建测试 Agent 使用的本地运行环境。
fn test_execution_env() -> Arc<LocalExecutionEnv> {
    Arc::new(
        LocalExecutionEnv::new(LocalExecutionEnvOptions {
            cwd: std::env::current_dir().expect("test working directory should be available"),
            shell_path: None,
            shell_env: HashMap::new(),
        })
        .expect("valid working directory should create environment"),
    )
}

/// 在指定事件上失败的 sync listener。
struct FailingListener {
    /// 已观察到的事件类型。
    seen: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl AgentEventListener for FailingListener {
    /// 验证 listener 错误会从 Agent::prompt 传播出来。
    async fn execute(&mut self, event: &AgentEvent<'_>) -> Result<(), AgentError> {
        let event_name = match event {
            AgentEvent::AgentStart => "agent_start",
            AgentEvent::TurnStart => "turn_start",
            AgentEvent::MessageStart { .. } => "message_start",
            AgentEvent::MessageUpdate { .. } => "message_update",
            AgentEvent::MessageEnd { .. } => "message_end",
            AgentEvent::TurnEnd { .. } => "turn_end",
            AgentEvent::AgentEnd { .. } => "agent_end",
            AgentEvent::ToolExecutionStart { .. } => "tool_execution_start",
            AgentEvent::ToolExecutionUpdate { .. } => "tool_execution_update",
            AgentEvent::ToolExecutionEnd { .. } => "tool_execution_end",
        };
        self.seen.lock().expect("seen mutex poisoned").push(event_name);
        if event_name == "message_end" {
            return Err(AgentError::Listener("boom".to_string()));
        }
        Ok(())
    }
}

/// 收集 proxy stream 事件的测试 sink。
#[derive(Default)]
struct CollectingSink {
    /// 已接收事件。
    events: Vec<AssistantMessageEvent>,
}

#[async_trait]
impl AssistantMessageEventSink for CollectingSink {
    /// 保存事件并返回当前消息快照。
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

#[tokio::test]
async fn agent_listener_error_is_propagated() {
    let model = test_model();
    let mut agent = Agent::new(AgentOptions {
        env: Some(test_execution_env()),
        model,
        stream_fn: Some(fixed_stream()),
        ..Default::default()
    })
    .expect("agent should be constructed");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut listener = FailingListener { seen: Arc::clone(&seen) };
    let mut listeners: [&mut dyn AgentEventListener; 1] = [&mut listener];

    let error = agent
        .prompt(vec![test_user_message("hello")], &mut listeners)
        .await
        .expect_err("listener error should propagate");

    assert!(matches!(error, AgentError::Listener(message) if message == "boom"));
    assert!(seen.lock().expect("seen mutex poisoned").contains(&"message_end"));
    assert!(!agent.state().is_streaming);
}

#[tokio::test]
async fn proxy_cancelled_before_request_emits_aborted_error() {
    let token = CancellationToken::new();
    token.cancel();
    let mut sink = CollectingSink::default();
    let error = tokio::time::timeout(Duration::from_secs(2), async {
        stream_proxy(
            test_model(),
            Context::default(),
            ProxyStreamOptions {
                auth_token: "test-token".to_string(),
                proxy_url: "http://127.0.0.1:9".to_string(),
                serializable: ProxySerializableStreamOptions::default(),
                cancellation_token: Some(token),
            },
            &mut sink,
        )
        .await
    })
    .await
    .expect("proxy stream should finish quickly")
    .expect_err("cancelled proxy stream should return an error");

    assert!(matches!(error, StreamError::Stream(message) if message.contains("aborted")));
    assert!(sink.events.is_empty());
}
