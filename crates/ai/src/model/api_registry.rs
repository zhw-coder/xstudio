//! 本模块提供核心 Provider 注册表，并在 `new()` 中注册内置 OpenAI Responses Provider。

use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use crate::model::{
    providers::{
        images::openai_images::OpenAIImagesProvider,
        texts::{openai_completions::OpenAICompletionsProvider, openai_responses::OpenAIResponsesProvider},
    },
    types::*,
};

/// AssistantMessage 事件消费端。
#[async_trait]
pub trait AssistantMessageEventSink: Send {
    /// 消费一个 AssistantMessage 事件并返回消息所有权。
    async fn emit(&mut self, event: AssistantMessageEvent) -> Result<AssistantMessage, StreamError>;
}

/// API Provider stream 函数统一 trait。
#[async_trait]
pub trait ApiProvider: Send + Sync {
    /// 获取当前 Provider 可用模型列表。
    async fn models(
        &self,
        provider: &str,
        base_url: &str,
        options: &StreamOptions,
        auth: &Auth,
    ) -> Result<Vec<Model>, StreamError>;

    /// 以流式事件形式调用文本/聊天模型。
    async fn stream(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError>;

    /// 简化版流式入口。
    async fn stream_simple(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError>;
}

/// Provider 注册表。
#[derive(Clone)]
pub struct ApiRegistry {
    /// 按 API 名称保存 provider。
    providers: HashMap<String, Arc<dyn ApiProvider>>,
}

static GLOBAL_API_REGISTRY: OnceLock<Arc<ApiRegistry>> = OnceLock::new();

impl ApiRegistry {
    /// 创建默认注册表并注册内置 OpenAI Provider。
    pub fn new() -> Self {
        let mut registry = Self { providers: HashMap::new() };
        registry.register(OpenAICompletionsProvider::API, Arc::new(OpenAICompletionsProvider::default()));
        registry.register(OpenAIResponsesProvider::API, Arc::new(OpenAIResponsesProvider::default()));
        registry.register(OpenAIImagesProvider::API, Arc::new(OpenAIImagesProvider::default()));
        registry
    }

    /// 全局静态单例注册表。
    pub fn global() -> &'static Arc<Self> {
        GLOBAL_API_REGISTRY.get_or_init(|| Arc::new(ApiRegistry::new()))
    }

    /// 使用内置 Provider 和启动期扩展初始化全局注册表。
    ///
    /// 必须在首次调用 `global` 前执行；同名扩展会覆盖内置 Provider。
    /// @param extensions API 标识到 Provider 实现的映射。
    pub fn global_with_extensions(
        extensions: HashMap<String, Arc<dyn ApiProvider>>,
    ) -> Result<&'static Arc<Self>, String> {
        let mut registry = Self::new();
        for (api, provider) in extensions {
            registry.register(api, provider);
        }
        GLOBAL_API_REGISTRY
            .set(Arc::new(registry))
            .map_err(|_| "ApiRegistry has already been initialized".to_string())?;
        Ok(GLOBAL_API_REGISTRY.get().expect("ApiRegistry must be available after initialization"))
    }

    /// 使用指定 API 标识注册一个 provider。
    ///
    /// 参数：
    /// - `api`: 用于查找 Provider 的 API 标识。
    /// - `provider`: 要注册的 Provider 实现。
    pub fn register(&mut self, api: impl Into<String>, provider: Arc<dyn ApiProvider>) {
        self.providers.insert(api.into(), provider);
    }

    /// 按 API 名查找 provider。
    pub fn get(&self, api: &str) -> Option<Arc<dyn ApiProvider>> {
        self.providers.get(api).cloned()
    }

    /// 返回所有已注册 provider。
    pub fn providers(&self) -> Vec<Arc<dyn ApiProvider>> {
        self.providers.values().cloned().collect()
    }

    /// 返回所有已注册 provider api。
    pub fn apis(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// 注销一个 API provider。
    pub fn unregister(&mut self, api: &str) {
        self.providers.remove(api);
    }

    /// 清空注册表。
    pub fn clear(&mut self) {
        self.providers.clear();
    }
}
