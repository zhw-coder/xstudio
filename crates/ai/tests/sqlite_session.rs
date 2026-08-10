//! SQLite session storage/repo 集成测试。

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use ai::{
    agent::{
        env::{ExecutionEnv, LocalExecutionEnv, LocalExecutionEnvOptions},
        harness::{
            compaction::find_cut_point,
            session::{repo::SqliteSessionRepo, storage::SqliteSessionStorage, Session},
            types::{
                CreateOptions, ListOptions, SessionCreateStorageOptions, SessionForkOptions, SessionRepo,
                SessionStorage,
            },
            AgentHarness, AgentHarnessOptions, AgentHarnessResources, AgentHarnessStreamOptions, NavigateTreeResult,
        },
        types::AgentMessage,
        QueueMode,
    },
    model::{
        empty_usage, now_millis, AssistantMessage, ContentBlock, Model, ModelCost, StopReason, TextContent, ToolCall,
        ToolResultMessage, UserContent, UserMessage,
    },
};

/// 构造测试数据库目录。
fn test_db_dir(name: &str) -> PathBuf {
    let root = PathBuf::from("../../.venv/data").join(format!("sqlite-session-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("should create sqlite test dir");
    root.join(name)
}

/// 构造存储层测试使用的 SQLite 数据库文件路径。
fn test_db_path(name: &str) -> PathBuf {
    test_db_dir(name).join("storage.sqlite")
}

/// 构造测试使用的 user 消息。
fn test_user_message(text: &str) -> AgentMessage {
    AgentMessage::User(UserMessage { content: UserContent::Text(text.to_string()), timestamp: now_millis() })
}

/// 构造测试使用的 assistant 消息。
fn test_assistant_message(text: &str) -> AgentMessage {
    AgentMessage::Assistant(AssistantMessage {
        content: vec![ContentBlock::Text(TextContent { text: text.to_string(), text_signature: None })],
        api: "test-api".to_string(),
        provider: "test-provider".to_string(),
        model: "test-model".to_string(),
        response_model: None,
        response_id: None,
        diagnostics: vec![],
        usage: empty_usage(),
        stop_reason: StopReason::Stop,
        error_message: None,
        timestamp: now_millis(),
    })
}

/// 构造带工具调用的 assistant 消息。
fn test_tool_call_message(tool_call_id: &str) -> AgentMessage {
    let mut message = match test_assistant_message("") {
        AgentMessage::Assistant(message) => message,
        _ => unreachable!("test assistant helper must create an assistant message"),
    };
    message.content = vec![ContentBlock::ToolCall(ToolCall {
        id: tool_call_id.to_string(),
        name: "read_file".to_string(),
        arguments: Default::default(),
        thought_signature: None,
    })];
    AgentMessage::Assistant(message)
}

/// 构造工具结果消息。
fn test_tool_result_message(tool_call_id: &str) -> AgentMessage {
    AgentMessage::ToolResult(ToolResultMessage {
        tool_call_id: tool_call_id.to_string(),
        tool_name: "read_file".to_string(),
        content: vec![ContentBlock::Text(TextContent { text: "ok".to_string(), text_signature: None })],
        details: None,
        is_error: false,
        timestamp: now_millis(),
    })
}

/// 构造仅用于会话导航测试的 Harness。
/// @param session 会话句柄。
async fn test_harness(session: Arc<Session>) -> AgentHarness {
    let cwd = session.with_metadata_guard().await.cwd.clone();
    AgentHarness::new(AgentHarnessOptions {
        env: Arc::new(
            LocalExecutionEnv::new(LocalExecutionEnvOptions {
                cwd: PathBuf::from(cwd),
                shell_path: None,
                shell_env: HashMap::new(),
            })
            .expect("should create test environment"),
        ),
        session,
        model: Model {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            api: "test-api".to_string(),
            provider: "test-provider".to_string(),
            base_url: "https://example.com".to_string(),
            reasoning: false,
            thinking_level_map: HashMap::new(),
            input: vec!["text".to_string()],
            cost: ModelCost { input: 0.0, output: 0.0, cache_read: 0.0, cache_write: 0.0 },
            context_window: 4096,
            max_tokens: 1024,
            headers: HashMap::new(),
            compat: None,
        },
        thinking_level: None,
        tools: vec![],
        active_tool_names: Some(vec![]),
        resources: AgentHarnessResources::default(),
        stream_options: AgentHarnessStreamOptions::default(),
        system_prompt: "test".to_string(),
        system_prompt_provider: None,
        auth_provider: None,
        steering_mode: QueueMode::OneAtATime,
        follow_up_mode: QueueMode::OneAtATime,
        listeners: vec![],
        before_agent_start_hooks: vec![],
        context_hooks: vec![],
        before_provider_request_hooks: vec![],
        before_provider_payload_hooks: vec![],
        after_provider_response_hooks: vec![],
        tool_call_hooks: vec![],
        tool_result_hooks: vec![],
    })
    .await
    .expect("should create test harness")
}

#[tokio::test]
async fn sqlite_storage_create_open_and_build_context() {
    let db_path = test_db_path("storage");
    let storage = SqliteSessionStorage::create(
        &db_path,
        SessionCreateStorageOptions {
            cwd: "/tmp/sqlite-storage".to_string(),
            session_id: "session-a".to_string(),
            name: String::new(),
            parent_session_path: None,
        },
    )
    .await
    .expect("should create sqlite storage");
    let session = Session::new(storage);

    const COMMIT_ID: &str = "0123456789012345678901234567890123456789";
    let user_id =
        session.append_message(test_user_message("hello"), Some(COMMIT_ID)).await.expect("should append user");
    let assistant_id =
        session.append_message(test_assistant_message("world"), None).await.expect("should append assistant");
    assert_eq!(user_id.as_bytes()[8], b'-');
    assert!(user_id.ends_with(COMMIT_ID));
    assert_eq!(user_id.len(), 49);
    assert_eq!(assistant_id.len(), 8);
    let label_id =
        session.append_label(user_id.clone(), Some("greeting".to_string())).await.expect("should append label");

    assert_eq!(session.get_leaf_id().await.as_deref(), Some(label_id.as_str()));
    assert_eq!(session.get_label(&user_id).await.as_deref(), Some("greeting"));
    assert_eq!(session.build_context().await.messages.len(), 2);

    let reopened = SqliteSessionStorage::open(&db_path, "session-a").await.expect("should reopen sqlite storage");
    let reopened_session = Session::new(reopened);

    assert_eq!(reopened_session.get_leaf_id().await.as_deref(), Some(label_id.as_str()));
    assert_eq!(reopened_session.get_label(&user_id).await.as_deref(), Some("greeting"));
    assert_eq!(reopened_session.build_context().await.messages.len(), 2);
}

/// 压缩切点不得从 tool result 开始，且压缩摘要必须进入模型上下文。
#[tokio::test]
async fn compaction_preserves_tool_call_pairs_and_summary_context() {
    let db_path = test_db_path("compaction-tool-pair");
    let storage = SqliteSessionStorage::create(
        &db_path,
        SessionCreateStorageOptions {
            cwd: "/tmp/sqlite-compaction-tool-pair".to_string(),
            session_id: "session-compaction-tool-pair".to_string(),
            name: String::new(),
            parent_session_path: None,
        },
    )
    .await
    .expect("should create sqlite storage");
    let session = Session::new(storage);

    session.append_message(test_user_message("use the tool"), None).await.expect("should append user");
    let tool_call_id = "call-1";
    session.append_message(test_tool_call_message(tool_call_id), None).await.expect("should append tool call");
    session.append_message(test_tool_result_message(tool_call_id), None).await.expect("should append tool result");
    session.append_message(test_assistant_message("done"), None).await.expect("should append assistant");

    let branch = session.get_branch(None).await;
    let cut_point = find_cut_point(&branch, 0, branch.len(), 3);
    assert!(matches!(
        branch.get(cut_point.first_kept_entry_index),
        Some(ai::agent::harness::types::SessionTreeEntry::Message {
            message: AgentMessage::Assistant(assistant),
            ..
        }) if assistant.content.iter().any(|block| matches!(block, ContentBlock::ToolCall(tool_call) if tool_call.id == tool_call_id))
    ));

    session
        .append_compaction(
            "tool work was summarized".to_string(),
            branch[cut_point.first_kept_entry_index].id().to_string(),
            100,
            None,
            Some(false),
        )
        .await
        .expect("should append compaction");
    let context = session.build_context().await;

    assert!(matches!(
        context.messages.first(),
        Some(AgentMessage::User(UserMessage { content: UserContent::Blocks(blocks), .. }))
            if matches!(blocks.as_slice(), [ContentBlock::Text(TextContent { text, .. })] if text.contains("tool work was summarized"))
    ));
    assert!(context.messages.iter().any(|message| matches!(
        message,
        AgentMessage::ToolResult(result) if result.tool_call_id == tool_call_id
    )));
    assert!(context.messages.iter().any(|message| matches!(
        message,
        AgentMessage::Assistant(assistant)
            if assistant.content.iter().any(|block| matches!(block, ContentBlock::ToolCall(tool_call) if tool_call.id == tool_call_id))
    )));
}

#[tokio::test]
async fn navigate_tree_with_current_user_leaf_withdraws_to_root() {
    let cwd = test_db_dir("navigate-current-user");
    let db_path = cwd.join("storage.sqlite");
    let storage = SqliteSessionStorage::create(
        &db_path,
        SessionCreateStorageOptions {
            cwd: cwd.display().to_string(),
            session_id: "session-navigation".to_string(),
            name: String::new(),
            parent_session_path: None,
        },
    )
    .await
    .expect("should create sqlite storage");
    let session = Arc::new(Session::new(storage));
    let checkpoint_env =
        LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd, shell_path: None, shell_env: HashMap::new() })
            .expect("should create checkpoint environment");
    let checkpoint_id = checkpoint_env.create_point().await.expect("should create checkpoint");
    session.append_message(test_user_message("撤回我"), Some(&checkpoint_id)).await.expect("should append user");
    let harness = test_harness(Arc::clone(&session)).await;

    let result: NavigateTreeResult =
        harness.navigate_tree(0, false, None, false, None).await.expect("should withdraw current user leaf");

    assert_eq!(session.get_leaf_id().await, None);
    assert_eq!(session.build_context_view().await.messages.len(), 0);
    assert_eq!(result.editor_text.as_deref(), Some("撤回我"));
}

/// 导航到带检查点的用户消息时回滚工作目录。
#[tokio::test]
async fn navigate_tree_resets_workspace_to_user_checkpoint() {
    let cwd = test_db_dir("navigate-checkpoint");
    let db_path = cwd.join("storage.sqlite");
    let storage = SqliteSessionStorage::create(
        &db_path,
        SessionCreateStorageOptions {
            cwd: cwd.display().to_string(),
            session_id: "session-navigation-checkpoint".to_string(),
            name: String::new(),
            parent_session_path: None,
        },
    )
    .await
    .expect("should create sqlite storage");
    let session = Arc::new(Session::new(storage));
    let file_path = cwd.join("state.txt");
    std::fs::write(&file_path, "before").expect("should write checkpoint state");
    let checkpoint_env = LocalExecutionEnv::new(LocalExecutionEnvOptions {
        cwd: cwd.clone(),
        shell_path: None,
        shell_env: HashMap::new(),
    })
    .expect("should create checkpoint environment");
    let checkpoint_id = checkpoint_env.create_point().await.expect("should create checkpoint");
    session.append_message(test_user_message("撤回我"), Some(&checkpoint_id)).await.expect("should append user");
    std::fs::write(&file_path, "after").expect("should write modified state");
    let harness = test_harness(Arc::clone(&session)).await;

    harness.navigate_tree(0, false, None, false, None).await.expect("should navigate to user checkpoint");

    assert_eq!(std::fs::read_to_string(file_path).expect("should read restored state"), "before");
}

#[tokio::test]
async fn sqlite_repo_list_fork_and_delete() {
    let db_dir = test_db_dir("repo");
    let repo = SqliteSessionRepo::new();
    repo.init(db_dir).await.expect("should initialize sqlite repository");
    let session = repo
        .create(CreateOptions {
            id: Some("session-a".to_string()),
            cwd: "/tmp/sqlite-repo".to_string(),
            parent_session_path: None,
        })
        .await
        .expect("should create sqlite session");
    session.get_storage().rename("source".to_string()).await.expect("should rename sqlite session");
    session.append_message(test_user_message("hello"), None).await.expect("should append user");
    session.append_message(test_assistant_message("world"), None).await.expect("should append assistant");

    let listed = repo
        .list(ListOptions { cwd: Some("/tmp/sqlite-repo".to_string()) })
        .await
        .expect("should list sqlite sessions");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "session-a");

    let forked = repo
        .fork(&session, SessionForkOptions { id: Some("session-b".to_string()), ..Default::default() })
        .await
        .expect("should fork sqlite session");
    assert_eq!(forked.build_context().await.messages.len(), 2);
    assert_eq!(forked.with_metadata_guard().await.name, "fork:source");
    assert!(forked.with_metadata_guard().await.parent_session_path.is_some());

    let all = repo.list(ListOptions::default()).await.expect("should list all sqlite sessions");
    assert_eq!(all.len(), 2);

    repo.delete(forked.get_metadata().await).await.expect("should delete forked session");
    let all = repo.list(ListOptions::default()).await.expect("should list remaining sqlite sessions");
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn sqlite_repo_list_filters_by_target_cwd() {
    /// list 接口过滤目标 cwd。
    const TARGET_CWD: &str = "/Users/apple/Documents/test";

    let db_dir = test_db_dir("repo-list");
    let db_path = db_dir.join("db.sqlite");
    let repo = SqliteSessionRepo::new();
    repo.init(db_dir).await.expect("should initialize sqlite repository");
    let target_session = repo
        .create(CreateOptions {
            id: Some("target-session".to_string()),
            cwd: TARGET_CWD.to_string(),
            parent_session_path: None,
        })
        .await
        .expect("should create target sqlite session");
    repo.rename(target_session.get_metadata().await, "Target Session".to_string())
        .await
        .expect("should rename target session");
    repo.create(CreateOptions {
        id: Some("other-session".to_string()),
        cwd: "/tmp/sqlite-repo-list".to_string(),
        parent_session_path: None,
    })
    .await
    .expect("should create other sqlite session");

    let listed = repo
        .list(ListOptions { cwd: Some(TARGET_CWD.to_string()) })
        .await
        .expect("should list target cwd sqlite sessions");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "target-session");
    assert_eq!(listed[0].name, "Target Session");
    assert_eq!(listed[0].cwd, TARGET_CWD);
    assert_eq!(
        listed[0].path,
        std::env::current_dir().expect("should resolve current test directory").join(db_path).display().to_string()
    );
    assert!(listed[0].parent_session_path.is_none());

    let all = repo.list(ListOptions::default()).await.expect("should list all sqlite sessions");
    assert_eq!(all.len(), 2);
}
