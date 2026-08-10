//! OpenAI Responses Provider 集成测试。
//!
//! 这个测试会通过 Agent 包装 provider，执行真实 OpenAI Responses `/responses` 请求，并打印事件与 hook 日志。
//!
//! 运行前请通过环境变量设置 `OPENAI_TEST_API_KEY`，可选设置 `OPENAI_TEST_BASE_URL` 和 `OPENAI_TEST_MODEL_ID`。

use ai::{
    agent::{
        env::{LocalExecutionEnv, LocalExecutionEnvOptions},
        Agent, AgentError, AgentEvent, AgentEventListener, AgentOptions, StreamFn,
    },
    model::{
        api_registry::AssistantMessageEventSink,
        stream::stream,
        types::{
            AssistantMessage, Auth, AuthProvider, Context, Model, ModelCost, ProviderPayloadCallback, ProviderResponse,
            ProviderResponseCallback, StreamError, StreamOptions, ThinkingLevel,
        },
    },
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

struct OpenAiTestStream;

struct StaticAuthProvider {
    api_key: String,
}

#[async_trait]
impl AuthProvider for StaticAuthProvider {
    async fn api_key_and_headers<'a>(&'a self, _model: &'a Model) -> Option<Auth> {
        Some(Auth { api_key: Some(self.api_key.clone()), headers: HashMap::new() })
    }
}

#[async_trait]
impl StreamFn for OpenAiTestStream {
    async fn stream<'a>(
        &'a self,
        model: &'a Model,
        context: Context,
        options: &'a StreamOptions,
        auth: &'a Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, AgentError> {
        stream(model, context, options, auth, sink).await.map_err(AgentError::from)
    }
}

struct LoggingPayloadCallback {
    logs: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ProviderPayloadCallback for LoggingPayloadCallback {
    async fn on_payload(&self, payload: serde_json::Value, model: &Model) -> Result<serde_json::Value, StreamError> {
        let payload_text = serde_json::to_string_pretty(&payload).unwrap_or_default();
        println!("[hook:on_payload] model={} payload={}", model.id, payload_text);
        self.logs.lock().unwrap().push("payload".to_string());
        Ok(payload)
    }
}

struct LoggingResponseCallback {
    logs: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ProviderResponseCallback for LoggingResponseCallback {
    async fn on_response(&self, response: ProviderResponse, model: &Model) -> Result<ProviderResponse, StreamError> {
        println!("[hook:on_response] model={} status={} headers={:?}", model.id, response.status, response.headers);
        self.logs.lock().unwrap().push("response".to_string());
        Ok(response)
    }
}

struct LoggingAgentListener {
    logs: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl AgentEventListener for LoggingAgentListener {
    async fn execute(&mut self, event: &AgentEvent<'_>) -> Result<(), AgentError> {
        println!("[agent-event] {:?}", event);
        self.logs.lock().unwrap().push(format!("event:{:?}", event));
        Ok(())
    }
}

#[tokio::test]
async fn openai_responses_provider_agent_event_hooks() {
    let api_key = std::env::var("OPENAI_TEST_API_KEY")
        .unwrap_or_else(|_| "sk-4a9896227bf49669f58a42fb12d266cb71275a58a2c861e43be4202014b36efa".to_string());
    let base_url =
        std::env::var("OPENAI_TEST_BASE_URL").unwrap_or_else(|_| "https://sub2api.inyuelan.com/v1".to_string());
    let model_id = std::env::var("OPENAI_TEST_MODEL_ID").unwrap_or_else(|_| "gpt-5.4".to_string());

    let payload_logs = Arc::new(Mutex::new(Vec::new()));
    let response_logs = Arc::new(Mutex::new(Vec::new()));
    let agent_logs = Arc::new(Mutex::new(Vec::new()));

    let payload_callback = Arc::new(LoggingPayloadCallback { logs: payload_logs.clone() });
    let response_callback = Arc::new(LoggingResponseCallback { logs: response_logs.clone() });
    let mut listener = LoggingAgentListener { logs: agent_logs.clone() };

    let mut stream_options = StreamOptions::default();
    stream_options.on_payload = Some(payload_callback);
    stream_options.on_response = Some(response_callback);
    stream_options.headers.insert("x-test-case".to_string(), "openai_responses_provider_agent_event_hooks".to_string());
    stream_options.max_tokens = Some(64);
    stream_options.temperature = Some(0.0);
    stream_options.reasoning = Some(ThinkingLevel::Minimal);

    let model = Model {
        id: model_id.clone(),
        name: "OpenAI Test Model".to_string(),
        api: "OpenAI-Responses".to_string(),
        provider: "openai".to_string(),
        base_url,
        reasoning: true,
        thinking_level_map: HashMap::new(),
        input: vec!["text".to_string()],
        cost: ModelCost { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0 },
        context_window: 40960,
        max_tokens: 1024,
        headers: HashMap::new(),
        compat: None,
    };

    let stream_fn: Box<dyn StreamFn> = Box::new(OpenAiTestStream);

    let mut agent = Agent::new(AgentOptions {
        env: Some(Arc::new(
            LocalExecutionEnv::new(LocalExecutionEnvOptions {
                cwd: std::env::current_dir().expect("test working directory should be available"),
                shell_path: None,
                shell_env: HashMap::new(),
            })
            .expect("valid working directory should create environment"),
        )),
        model,
        stream_fn: Some(stream_fn),
        stream_options,
        auth_provider: Some(Arc::new(StaticAuthProvider { api_key })),
        ..Default::default()
    })
    .expect("agent should construct");

    let mut listeners: [&mut dyn AgentEventListener; 1] = [&mut listener];

    let result = agent
        .prompt_text("Hello from OpenAI Responses provider agent test", Vec::new(), &mut listeners)
        .await
        .expect("agent prompt should succeed");

    println!("[agent-result] messages={:?}", result);
    println!(
        "[test-summary] payload_hooks={:?} response_hooks={:?} agent_events={:?}",
        payload_logs.lock().unwrap(),
        response_logs.lock().unwrap(),
        agent_logs.lock().unwrap()
    );

    assert!(payload_logs.lock().unwrap().contains(&"payload".to_string()), "expected payload hook to run");
    assert!(response_logs.lock().unwrap().contains(&"response".to_string()), "expected response hook to run");
    assert!(!agent_logs.lock().unwrap().is_empty(), "expected agent event listener to observe lifecycle events");
}
