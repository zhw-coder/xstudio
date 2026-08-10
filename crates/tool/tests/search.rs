//! SearchTool 与搜索配置集成测试。

use ai::agent::AgentTool;
use serde_json::json;
use std::sync::Mutex;

use tool::{SearchRegistry, SearchTool};

/// 串行化访问全局搜索注册表的测试。
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// 验证仅配置专业搜索时仍在 Schema 中注册 general 实体。
#[test]
fn new_registers_general_engine_when_missing() {
    let _guard = TEST_LOCK.lock().expect("test lock should not be poisoned");
    let tool = SearchTool::new();
    tool.init(json!({"pubmed":{"url":"https://example.com","limit":10}}))
        .expect("valid PubMed config");
    let definition = tool.definition();
    let domains = definition.parameters["properties"]["domain"]["enum"]
        .as_array()
        .expect("domain enum should be an array");

    assert!(domains.contains(&json!("biomedical")));
    assert!(domains.contains(&json!("general")));
}

/// 验证 Schema 仅将 query 作为必填参数，并保留 general 领域。
#[test]
fn parameters_allow_omitting_domain() {
    let _guard = TEST_LOCK.lock().expect("test lock should not be poisoned");
    let parameters = SearchTool::new().definition().parameters;

    assert_eq!(parameters["required"], json!(["query"]));
    assert!(parameters["properties"]["domain"]["enum"]
        .as_array()
        .expect("domain enum should be an array")
        .contains(&json!("general")));
}

/// 验证相同配置不会改变 SearchTool 的可用领域。
#[test]
fn build_skips_unchanged_configs() {
    let _guard = TEST_LOCK.lock().expect("test lock should not be poisoned");
    let tool = SearchTool::new();
    let configs = json!({"pubmed":{"url":"https://example.com","limit":10}});

    tool.init(configs.clone()).expect("valid PubMed config");
    tool.init(configs).expect("unchanged config is valid");

    let definition = tool.definition();
    let domains = definition.parameters["properties"]["domain"]["enum"]
        .as_array()
        .expect("domain enum should be an array");
    assert!(domains.contains(&json!("biomedical")));
}

/// 验证新增通用搜索实体暴露在客户端配置入口中。
#[test]
fn engines_include_new() {
    let _guard = TEST_LOCK.lock().expect("test lock should not be poisoned");
    let engines = SearchRegistry::global().engines();
    let general = engines
        .iter()
        .find(|engines| engines.first() == Some(&"general".to_string()))
        .expect("general engines should exist");

    assert!(general.contains(&"baidu".to_string()));
    assert!(general.contains(&"bing".to_string()));
    assert!(SearchRegistry::global().get("baidu").is_some());
    assert!(SearchRegistry::global().get("bing").is_some());
    assert!(general.contains(&"searxng".to_string()));
    assert!(SearchRegistry::global().get("searxng").is_some());
}

/// 验证默认参数 JSON 包含密钥、地址与结果数量默认值。
#[test]
fn parameters_return_default_values() {
    let _guard = TEST_LOCK.lock().expect("test lock should not be poisoned");
    assert_eq!(
        SearchRegistry::global()
            .get("baidu")
            .expect("Baidu should be registered")
            .parameters()
            .expect("Baidu parameters should be readable"),
        json!({
            "url": "https://qianfan.baidubce.com/v2/ai_search/web_search",
            "key": "",
            "limit": 10,
        })
    );
    assert_eq!(
        SearchRegistry::global()
            .get("searxng")
            .expect("SearXNG should be registered")
            .parameters()
            .expect("SearXNG parameters should be readable"),
        json!({
            "urls": ["https://127.0.0.1:8080"],
            "limit": 10,
        })
    );
}

/// 验证反序列化时拒绝缺失的必填参数字段。
#[test]
fn from_parameters_rejects_missing_fields() {
    let _guard = TEST_LOCK.lock().expect("test lock should not be poisoned");
    assert!(SearchRegistry::global()
        .init(std::collections::HashMap::from([(
            "github".to_string(),
            json!({}),
        )]))
        .is_err());
}
