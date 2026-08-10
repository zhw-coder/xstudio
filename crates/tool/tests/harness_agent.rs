//! Harness Agent 集成测试。
//!
//! 这个测试会通过 `AgentHarness` 包装 OpenAI Responses Provider，使用 SQLite 文件版会话存储，
//! 执行真实请求，并打印 harness 事件、provider hook 日志以及使用的模型和 key 信息。
//!
//! 运行前请通过环境变量设置 `OPENAI_TEST_API_KEY`，可选设置 `OPENAI_TEST_BASE_URL` 和 `OPENAI_TEST_MODEL_ID`。

use ai::{
    agent::{
        agent::QueueMode,
        env::local::{LocalExecutionEnv, LocalExecutionEnvOptions},
        harness::{
            agent_harness::{AgentHarness, AgentHarnessOptions},
            session::{storage::sqlite::SqliteSessionStorage, Session},
            types::{
                AfterProviderResponseHook, AgentHarnessEvent, AgentHarnessOwnEvent,
                AgentHarnessResources, AgentHarnessStreamOptions, BeforeProviderPayloadHook,
                SessionCreateStorageOptions, SessionStorage,
            },
        },
        types::{AgentError, MHook},
    },
    model::{
        types::{Auth, AuthProvider, Model, ModelCost},
        ThinkingLevel,
    },
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

// ---------------------------------------------------------------------------
// 测试辅助：静态认证提供者
// ---------------------------------------------------------------------------

/// 固定返回测试 api key 的认证提供者。
struct StaticAuthProvider {
    /// 测试 API key。
    api_key: String,
}

#[async_trait]
impl AuthProvider for StaticAuthProvider {
    async fn api_key_and_headers<'a>(&'a self, _model: &'a Model) -> Option<Auth> {
        Some(Auth {
            api_key: Some(self.api_key.clone()),
            headers: HashMap::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// 测试辅助：Harness 事件日志监听器
// ---------------------------------------------------------------------------

/// 收集并打印 Harness 事件的监听器。
struct HarnessLoggingListener {
    /// 已观察到的事件名称。
    logs: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl<'a> MHook<AgentHarnessEvent<'a>, ()> for HarnessLoggingListener {
    async fn execute(&self, event: &mut AgentHarnessEvent<'a>) -> Result<(), AgentError> {
        let name = match event {
            AgentHarnessEvent::Agent(agent_event) => {
                let name = format!("agent:{:?}", agent_event);
                println!("[harness-event] {name}");
                name
            }
            AgentHarnessEvent::Harness(harness_event) => {
                let name = match harness_event {
                    AgentHarnessOwnEvent::BeforeAgentStart {
                        prompt,
                        system_prompt,
                        ..
                    } => {
                        format!("harness:before_agent_start prompt=\"{prompt}\" system_prompt=\"{system_prompt}\"")
                    }
                    AgentHarnessOwnEvent::Context { messages } => {
                        format!("harness:context messages_count={}", messages.len())
                    }
                    AgentHarnessOwnEvent::BeforeProviderRequest { model, .. } => {
                        format!("harness:before_provider_request model={}", model.id)
                    }
                    AgentHarnessOwnEvent::BeforeProviderPayload { model, .. } => {
                        format!("harness:before_provider_payload model={}", model.id)
                    }
                    AgentHarnessOwnEvent::AfterProviderResponse { status, headers } => {
                        format!(
                            "harness:after_provider_response status={status} headers={headers:?}"
                        )
                    }
                    AgentHarnessOwnEvent::SavePoint {
                        had_pending_mutations,
                    } => {
                        format!("harness:save_point had_pending_mutations={had_pending_mutations}")
                    }
                    AgentHarnessOwnEvent::Settled { next_turn_count } => {
                        format!("harness:settled next_turn_count={next_turn_count}")
                    }
                    AgentHarnessOwnEvent::QueueUpdate {
                        steer,
                        follow_up,
                        next_turn,
                    } => {
                        format!(
                            "harness:queue_update steer={} follow_up={} next_turn={}",
                            steer.len(),
                            follow_up.len(),
                            next_turn.len()
                        )
                    }
                    AgentHarnessOwnEvent::Abort {
                        cleared_steer,
                        cleared_follow_up,
                    } => {
                        format!(
                            "harness:abort cleared_steer={} cleared_follow_up={}",
                            cleared_steer.len(),
                            cleared_follow_up.len()
                        )
                    }
                    AgentHarnessOwnEvent::ModelSelect {
                        model,
                        previous_model,
                        source,
                    } => {
                        format!(
                            "harness:model_select model={} previous={:?} source={:?}",
                            model.id,
                            previous_model.map(|m| m.id.as_str()),
                            source
                        )
                    }
                    AgentHarnessOwnEvent::ThinkingLevelSelect {
                        level,
                        previous_level,
                    } => {
                        format!("harness:thinking_level_select level={level:?} previous={previous_level:?}")
                    }
                    AgentHarnessOwnEvent::ResourcesUpdate {
                        resources,
                        previous_resources,
                    } => {
                        format!(
                            "harness:resources_update skills={} previous_skills={}",
                            resources.skills.len(),
                            previous_resources.skills.len()
                        )
                    }
                    AgentHarnessOwnEvent::ToolCall {
                        tool_call_id,
                        tool_name,
                        ..
                    } => {
                        format!("harness:tool_call id={tool_call_id} name={tool_name}")
                    }
                    AgentHarnessOwnEvent::ToolResult {
                        tool_call_id,
                        tool_name,
                        is_error,
                        ..
                    } => {
                        format!("harness:tool_result id={tool_call_id} name={tool_name} is_error={is_error}")
                    }
                };
                println!("[harness-event] {name}");
                name
            }
        };
        self.logs
            .lock()
            .expect("harness logging lock poisoned")
            .push(name);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 测试辅助：Provider payload hook
// ---------------------------------------------------------------------------

/// 在 Provider 发送 payload 前打印日志的 hook。
struct LoggingPayloadHook {
    /// hook 执行记录。
    logs: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl<'a> MHook<AgentHarnessOwnEvent<'a>, Option<Value>> for LoggingPayloadHook {
    async fn execute(
        &self,
        event: &mut AgentHarnessOwnEvent<'a>,
    ) -> Result<Option<Value>, AgentError> {
        if let AgentHarnessOwnEvent::BeforeProviderPayload { model, payload } = event {
            let payload_text = serde_json::to_string_pretty(payload).unwrap_or_default();
            println!(
                "[hook:before_provider_payload] model={} payload={}",
                model.id, payload_text
            );
            println!(
                "[hook:before_provider_payload] model_name={} provider={}",
                model.name, model.provider
            );
            self.logs
                .lock()
                .expect("payload hook lock poisoned")
                .push("payload".to_string());
        }
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// 测试辅助：Provider response hook
// ---------------------------------------------------------------------------

/// 在 Provider 返回响应后打印日志的 hook。
struct LoggingResponseHook {
    /// hook 执行记录。
    logs: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl<'a> MHook<AgentHarnessOwnEvent<'a>, ()> for LoggingResponseHook {
    async fn execute(&self, event: &mut AgentHarnessOwnEvent<'a>) -> Result<(), AgentError> {
        if let AgentHarnessOwnEvent::AfterProviderResponse { status, headers } = event {
            println!("[hook:after_provider_response] status={status} headers={headers:?}");
            self.logs
                .lock()
                .expect("response hook lock poisoned")
                .push("response".to_string());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 测试：Harness Agent 使用 SQLite 文件存储 + 事件监听 + hook 日志
// ---------------------------------------------------------------------------

#[tokio::test]
async fn harness_agent_sqlite_storage_with_event_logging() {
    // 从环境变量读取模型和 key 配置，打印使用信息。
    let api_key = std::env::var("OPENAI_TEST_API_KEY").unwrap_or_else(|_| {
        "36kCqRPmAngRsHLFlwIpZ8MALV745HCQL6daiuF95OzQkFtPauBsv8uLYDpCYInfs".to_string()
        //sk-4a9896227bf49669f58a42fb12d266cb71275a58a2c861e43be4202014b36efa
    });
    let base_url = std::env::var("OPENAI_TEST_BASE_URL")
        .unwrap_or_else(|_| "https://api.stepfun.com/step_plan/v1".to_string()); //https://sub2api.inyuelan.com/v1
    let model_id =
        std::env::var("OPENAI_TEST_MODEL_ID").unwrap_or_else(|_| "step-3.5-flash".to_string()); //gpt-5.6-terra

    // 打印使用的模型和 key（key 仅显示前后各 4 位）。
    let masked_key = if api_key.len() > 8 {
        format!("{}...{}", &api_key[..4], &api_key[api_key.len() - 4..])
    } else {
        "***".to_string()
    };
    println!("[test-config] model_id={model_id} base_url={base_url} api_key={masked_key}");

    // 准备临时目录用于 SQLite 文件存储。
    let data_dir = PathBuf::from("../../.venv/data"); //Users/PaPa/Documents/test
    let temp_dir = data_dir;
    std::fs::create_dir_all(&temp_dir).expect("should create data dir");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    let session_file = temp_dir.join(format!(
        "harness-test-{}-{timestamp}.sqlite",
        std::process::id()
    ));
    println!("[test-config] session_file={}", session_file.display());

    // 旧 JSONL 文件版会话存储保留为注释，便于对照回退。
    // let session_file = temp_dir.join("test-session.jsonl");
    // let storage = JsonlSessionStorage::create(
    //     &session_file,
    //     SessionCreateStorageOptions {
    //         cwd: temp_dir.display().to_string(),
    //         session_id: "test-session-001".to_string(),
    //         parent_session_path: None,
    //     },
    // )
    // .await
    // .expect("should create jsonl session storage");

    // 构建 SQLite 文件版会话存储。
    let storage = SqliteSessionStorage::create(
        &session_file,
        SessionCreateStorageOptions {
            cwd: temp_dir.display().to_string(),
            name: "Harness Test Session".to_string(),
            session_id: "test-session-001".to_string(),
            parent_session_path: None,
        },
    )
    .await
    .expect("should create sqlite session storage");
    let session = Arc::new(Session::new(storage));

    // 构建本地执行环境。
    let env = Arc::new(
        LocalExecutionEnv::new(LocalExecutionEnvOptions {
            cwd: PathBuf::from(temp_dir.display().to_string()),
            shell_path: None,
            shell_env: HashMap::new(),
        })
        .expect("valid working directory should create environment"),
    );

    // 构建测试模型。
    let model = Model {
        id: model_id,
        name: "Harness Test Model".to_string(),
        api: "OpenAI-Completions".to_string(),
        provider: "openai".to_string(),
        base_url,
        reasoning: true,
        thinking_level_map: HashMap::new(),
        input: vec!["text".to_string()],
        cost: ModelCost {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_write: 0.0,
        },
        context_window: 40960,
        max_tokens: 1024,
        headers: HashMap::new(),
        compat: None,
    };

    // 准备日志收集。
    let harness_logs = Arc::new(Mutex::new(Vec::new()));
    let payload_logs = Arc::new(Mutex::new(Vec::new()));
    let response_logs = Arc::new(Mutex::new(Vec::new()));

    // 构建 Harness 事件监听器。
    let harness_listener = Arc::new(HarnessLoggingListener {
        logs: harness_logs.clone(),
    });

    // 构建 Provider hooks。
    let payload_hook: Box<BeforeProviderPayloadHook> = Box::new(LoggingPayloadHook {
        logs: payload_logs.clone(),
    });
    let response_hook: Box<AfterProviderResponseHook> = Box::new(LoggingResponseHook {
        logs: response_logs.clone(),
    });

    // 构建 stream options。
    let stream_options = AgentHarnessStreamOptions {
        headers: {
            let mut headers = HashMap::new();
            headers.insert(
                "x-test-case".to_string(),
                "harness_agent_sqlite_storage".to_string(),
            );
            headers
        },
        ..Default::default()
    };

    // 使用指定 SearXNG 实例创建全部内置工具，并作为通用搜索实体。
    tool::ToolRegistry::global()
        .init(HashMap::from([(
            "search".to_string(),
            json!({"searxng":{"urls":["https://test.ximiplay.com:8888"],"limit":3}}),
        )]))
        .expect("valid SearXNG config");
    let tools = tool::ToolRegistry::global().tools();

    // 构造 AgentHarness。
    let harness = AgentHarness::new(AgentHarnessOptions {
        env,
        session,
        model,
        thinking_level: Some(ThinkingLevel::High),
        tools,
        active_tool_names: Some(vec![
            "search".to_string(),
            "fetch".to_string(),
            "bash".to_string(),
            "edit".to_string(),
            "find".to_string(),
            "grep".to_string(),
            // "ls".to_string(),
            "read".to_string(),
            "write".to_string(),
        ]),
        resources: AgentHarnessResources::default(),
        stream_options,
        system_prompt: "你是一个个人助手。".to_string(),
        system_prompt_provider: None,
        auth_provider: Some(Arc::new(StaticAuthProvider { api_key })),
        steering_mode: QueueMode::OneAtATime,
        follow_up_mode: QueueMode::OneAtATime,
        listeners: vec![harness_listener],
        before_agent_start_hooks: vec![],
        context_hooks: vec![],
        before_provider_request_hooks: vec![],
        before_provider_payload_hooks: vec![payload_hook],
        after_provider_response_hooks: vec![response_hook],
        tool_call_hooks: vec![],
        tool_result_hooks: vec![],
    })
    .await
    .expect("harness should construct");

    harness
        .append_session_name("自我介绍")
        .await
        .expect("harness should construct");

    // 打印初始 phase。
    let phase = harness.phase().await;
    println!("[harness] initial phase={phase:?}");

    // 执行 prompt。
    let result = harness.prompt("把饮料从冰箱中拿出来，需要几步", None).await;

    match result {
        Ok(message) => {
            let text = message
                .content
                .iter()
                .filter_map(|block| match block {
                    ai::model::types::ContentBlock::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            println!("[harness-result] assistant_text=\"{text}\"");
            println!(
                "[harness-result] model={} provider={}",
                message.model, message.provider
            );
        }
        Err(error) => {
            // 即使请求失败，仍然打印已收集的日志以便排查。
            eprintln!("[harness-error] {error}");
        }
    }

    // 打印 summary。
    println!(
        "[test-summary] harness_events={:?} payload_hooks={:?} response_hooks={:?}",
        harness_logs.lock().unwrap(),
        payload_logs.lock().unwrap(),
        response_logs.lock().unwrap()
    );

    // 验证事件和 hook 已执行。
    let harness_events = harness_logs.lock().unwrap();
    assert!(
        harness_events
            .iter()
            .any(|event| event.contains("before_agent_start")),
        "expected before_agent_start event"
    );
    assert!(
        harness_events
            .iter()
            .any(|event| event.contains("before_provider_request")),
        "expected before_provider_request event"
    );
    assert!(
        harness_events
            .iter()
            .any(|event| event.contains("save_point")),
        "expected save_point event"
    );
    assert!(
        harness_events.iter().any(|event| event.contains("settled")),
        "expected settled event"
    );

    assert!(
        payload_logs
            .lock()
            .unwrap()
            .contains(&"payload".to_string()),
        "expected payload hook to run"
    );
    assert!(
        response_logs
            .lock()
            .unwrap()
            .contains(&"response".to_string()),
        "expected response hook to run"
    );

    // 验证 session 文件已写入。
    // assert!(session_file.exists(), "expected jsonl session file to exist");
    // let content = std::fs::read_to_string(&session_file).expect("should read session file");
    // let lines: Vec<&str> = content.lines().collect();
    // assert!(lines.len() >= 2, "expected at least header + 1 entry in jsonl file");
    // println!("[test-validation] session file has {} lines", lines.len());
    assert!(
        session_file.exists(),
        "expected sqlite session file to exist"
    );
    let metadata =
        std::fs::metadata(&session_file).expect("should read sqlite session file metadata");
    assert!(
        metadata.len() > 0,
        "expected sqlite session file to be non-empty"
    );
    println!(
        "[test-validation] session file has {} bytes",
        metadata.len()
    );

    // 清理临时目录。
    // let _ = std::fs::remove_dir_all(&temp_dir);
}
