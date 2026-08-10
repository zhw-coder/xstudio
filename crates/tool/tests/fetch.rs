//! FetchTool 集成测试。

use ai::{
    agent::{
        env::{LocalExecutionEnv, LocalExecutionEnvOptions},
        AgentTool,
    },
    model::ContentBlock,
};
use serde_json::json;
use std::path::PathBuf;
use tokio::{io::AsyncWriteExt, net::TcpListener};
use tool::FetchTool;

/// 创建不依赖当前工作目录的测试执行环境。
fn test_execution_env() -> LocalExecutionEnv {
    LocalExecutionEnv::new(LocalExecutionEnvOptions {
        cwd: PathBuf::from("."),
        ..Default::default()
    })
    .expect("valid working directory should create environment")
}

/// 验证工具 Schema 要求非空 urls 数组。
#[test]
fn definition_requires_urls_array() {
    let parameters = FetchTool::new().definition().parameters;

    assert_eq!(parameters["required"], json!(["urls"]));
    assert_eq!(parameters["properties"]["urls"]["type"], json!("array"));
}

/// 验证 URL 数组中存在非法条目时会返回参数错误。
#[tokio::test]
async fn execute_rejects_invalid_url_items() {
    let params = json!({ "urls": ["https://example.com", ""] });
    let tool_call_id = "test-call".to_string();
    let env = test_execution_env();
    let error = FetchTool::new()
        .execute(&env, &tool_call_id, &params, None)
        .await
        .expect_err("empty URL item should be rejected");

    assert_eq!(error.to_string(), "urls items must be non-empty strings");
}

/// 验证优先提取 article 内容并去除空白。
#[tokio::test]
async fn execute_prefers_article_content() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("server should accept request");
        let body = "<html><body><nav>导航</nav><article><h1>标题</h1><p>正文 内容</p></article><footer>页脚</footer></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("server should write response");
    });
    let tool_call_id = "test-call".to_string();
    let env = test_execution_env();
    let result = FetchTool::new()
        .execute(
            &env,
            &tool_call_id,
            &json!({ "urls": [format!("http://{address}")] }),
            None,
        )
        .await
        .expect("fetch should succeed");
    let text = result
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .expect("result should contain text");

    assert!(text.contains("标题 正文 内容"));
    assert!(!text.contains("导航"));
    assert!(!text.contains("页脚"));
}

/// 验证某个 URL 失败时，后续本机 URL 仍会执行并返回结果。
#[tokio::test]
async fn execute_continues_after_failed_url() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("server should accept request");
        let body = "<html><body><main>本机网页正文</main></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("server should write response");
    });
    let tool_call_id = "test-call".to_string();
    let params = json!({ "urls": ["invalid-url", format!("http://{address}")] });
    let env = test_execution_env();

    let result = FetchTool::new()
        .execute(&env, &tool_call_id, &params, None)
        .await
        .expect("partial failures should not fail the tool call");
    let text = result
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .expect("result should contain text");

    assert!(text.contains("Fetch failed"));
    assert!(text.contains("本机网页正文"));
    assert_eq!(result.details["results"][0]["status"], json!(500));
    assert_eq!(result.details["results"][1]["status"], json!(200));
}
