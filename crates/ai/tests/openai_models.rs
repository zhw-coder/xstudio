//! OpenAI Responses Provider 模型列表接口测试。
//!
//! 默认测试只验证 `ApiProvider::models` 接口可通过注册表调用。
//! 真实 OpenAI `/models` 请求测试默认忽略，运行时需设置 `OPENAI_TEST_API_KEY`。

use ai::model::{
    api_registry::{ApiProvider, ApiRegistry, AssistantMessageEventSink},
    providers::texts::openai_responses::OpenAIResponsesProvider,
    types::{AssistantMessage, Auth, Context, Model, ModelCost, StreamError, StreamOptions},
};
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

/// 用于验证注册表 models 入口的测试 Provider。
#[derive(Debug, Default)]
struct TestModelsProvider;

#[async_trait]
impl ApiProvider for TestModelsProvider {
    /// 返回固定模型列表。
    async fn models(
        &self,
        _provider: &str,
        _base_url: &str,
        _options: &StreamOptions,
        _auth: &Auth,
    ) -> Result<Vec<Model>, StreamError> {
        Ok(vec![test_model()])
    }

    /// 本测试不覆盖 stream。
    async fn stream(
        &self,
        _model: &Model,
        _context: Context,
        _options: &StreamOptions,
        _auth: &Auth,
        _sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        Err(StreamError::Stream("stream is not used in models test".to_string()))
    }

    /// 本测试不覆盖 stream_simple。
    async fn stream_simple(
        &self,
        _model: &Model,
        _context: Context,
        _options: &StreamOptions,
        _auth: &Auth,
        _sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        Err(StreamError::Stream("stream_simple is not used in models test".to_string()))
    }
}

#[tokio::test]
async fn api_provider_models_can_be_called_from_registry() {
    let mut registry = ApiRegistry::new();
    registry.register("test-models", Arc::new(TestModelsProvider));

    let provider = registry.get("test-models").expect("test provider should be registered");
    let models = provider
        .models("test-provider", "https://example.invalid/v1", &StreamOptions::default(), &Auth::default())
        .await
        .expect("models should return fixed test list");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "test-model");
    assert_eq!(models[0].api, "test-models");
}

#[tokio::test]
#[ignore = "requires OPENAI_TEST_API_KEY and performs a real OpenAI /models request"]
async fn openai_responses_provider_lists_models_from_api() {
    let api_key = std::env::var("OPENAI_TEST_API_KEY")
        .unwrap_or_else(|_| "sk-4a9896227bf49669f58a42fb12d266cb71275a58a2c861e43be4202014b36efa".to_string());
    let base_url =
        std::env::var("OPENAI_TEST_BASE_URL").unwrap_or_else(|_| "https://sub2api.inyuelan.com/v1".to_string());

    let provider = OpenAIResponsesProvider;
    let provider_name = "openai";
    let options = StreamOptions::default();
    let auth = Auth { api_key: Some(api_key), headers: HashMap::new() };

    let models =
        provider.models(provider_name, &base_url, &options, &auth).await.expect("OpenAI models request should succeed");

    assert!(!models.is_empty(), "expected at least one OpenAI model");
    println!("resut={:?}", models);
    // assert!(models.iter().all(|model| model.provider == provider_name));
    // assert!(models.iter().all(|model| model.base_url == base_url));
}

/// 构造固定测试模型。
fn test_model() -> Model {
    Model {
        id: "test-model".to_string(),
        name: "Test Model".to_string(),
        api: "test-models".to_string(),
        provider: "test-provider".to_string(),
        base_url: "https://sub2api.inyuelan.com/v1".to_string(),
        reasoning: false,
        thinking_level_map: HashMap::new(),
        input: vec!["text".to_string()],
        cost: ModelCost::default(),
        context_window: 0,
        max_tokens: 0,
        headers: HashMap::new(),
        compat: None,
    }
}
