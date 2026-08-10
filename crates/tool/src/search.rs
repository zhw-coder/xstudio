//! 多搜索引擎聚合 Agent 工具。

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use ai::{
    agent::{env::ExecutionEnv, AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook},
    model::{ContentBlock, TextContent, Tool},
};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;

pub mod baidu;
pub mod bing;
pub mod brave;
pub mod core;
pub mod crossref;
pub mod duckduckgo;
pub mod github;
pub mod pubmed;
pub mod searxng;
pub mod stackoverflow;
pub mod tavily;

use baidu::BaiduSearch;
use bing::BingSearch;
use brave::BraveSearch;
use core::CoreSearch;
use crossref::CrossrefSearch;
use duckduckgo::DuckDuckGoSearch;
use github::GitHubSearch;
use pubmed::PubMedSearch;
use searxng::SearxngSearch;
use stackoverflow::StackOverflowSearch;
use tavily::TavilySearch;

/// 搜索实体注册表。
#[derive(Clone)]
pub struct SearchRegistry {
    /// 按实体名称保存已注册搜索实体。
    engines: HashMap<String, Arc<dyn SearchEngine>>,
}

/// 全局静态搜索实体注册表。
static GLOBAL_SEARCH_REGISTRY: OnceLock<Arc<SearchRegistry>> = OnceLock::new();

impl SearchRegistry {
    /// 创建并注册全部内置搜索实体。
    pub fn new() -> Self {
        let mut registry = Self {
            engines: HashMap::new(),
        };
        registry.register(BaiduSearch::name(), Arc::new(BaiduSearch::new()));
        registry.register(BingSearch::name(), Arc::new(BingSearch::new()));
        registry.register(BraveSearch::name(), Arc::new(BraveSearch::new()));
        registry.register(CoreSearch::name(), Arc::new(CoreSearch::new()));
        registry.register(CrossrefSearch::name(), Arc::new(CrossrefSearch::new()));
        registry.register(DuckDuckGoSearch::name(), Arc::new(DuckDuckGoSearch::new()));
        registry.register(GitHubSearch::name(), Arc::new(GitHubSearch::new()));
        registry.register(PubMedSearch::name(), Arc::new(PubMedSearch::new()));
        registry.register(SearxngSearch::name(), Arc::new(SearxngSearch::new()));
        registry.register(
            StackOverflowSearch::name(),
            Arc::new(StackOverflowSearch::new()),
        );
        registry.register(TavilySearch::name(), Arc::new(TavilySearch::new()));
        registry
    }

    /// 返回全局静态单例注册表。
    pub fn global() -> &'static Arc<Self> {
        GLOBAL_SEARCH_REGISTRY.get_or_init(|| Arc::new(Self::new()))
    }

    /// 使用内置搜索实体和启动期扩展初始化全局注册表。
    ///
    /// 必须在首次调用 `global` 前执行；同名扩展会覆盖内置搜索实体。
    /// @param extensions 搜索实体名称到搜索实体实例的映射。
    pub fn global_with_extensions(
        extensions: HashMap<String, Arc<dyn SearchEngine>>,
    ) -> Result<&'static Arc<Self>, String> {
        let mut registry = Self::new();
        for (name, engine) in extensions {
            registry.register(name, engine);
        }
        GLOBAL_SEARCH_REGISTRY
            .set(Arc::new(registry))
            .map_err(|_| "SearchRegistry has already been initialized".to_string())?;
        Ok(GLOBAL_SEARCH_REGISTRY
            .get()
            .expect("SearchRegistry must be available after initialization"))
    }

    /// 注册搜索实体。
    /// @param name 搜索实体名称。
    /// @param engine 搜索实体实例。
    pub fn register(&mut self, name: impl Into<String>, engine: Arc<dyn SearchEngine>) {
        self.engines.insert(name.into(), engine);
    }

    /// 返回按领域分组的全部已注册搜索实体名称；每行首项为领域。
    pub fn engines(&self) -> Vec<Vec<String>> {
        let mut domains: HashMap<String, Vec<String>> = HashMap::new();
        for (name, engine) in &self.engines {
            domains
                .entry(engine.domain().to_string())
                .or_default()
                .push(name.clone());
        }
        let mut engines = domains
            .into_iter()
            .map(|(domain, mut names)| {
                names.sort_unstable();
                let mut group = vec![domain];
                group.extend(names);
                group
            })
            .collect::<Vec<_>>();
        engines.sort_unstable_by(|left, right| left[0].cmp(&right[0]));
        engines
    }

    /// 获取指定已注册搜索实体。
    /// @param name 搜索实体名称。
    pub fn get(&self, name: &str) -> Option<Arc<dyn SearchEngine>> {
        self.engines.get(name).map(Arc::clone)
    }

    /// 使用配置初始化搜索实体并构建按领域索引的运行时映射。
    /// @param configs 搜索实体名称到参数的映射。
    pub fn init(
        &self,
        configs: HashMap<String, Value>,
    ) -> Result<HashMap<String, Arc<dyn SearchEngine>>, AgentToolError> {
        let mut engine_map = HashMap::new();
        for (name, parameters) in configs {
            let engine = self
                .get(&name)
                .ok_or_else(|| AgentToolError::Message(format!("Unknown search engine: {name}")))?;
            engine.init(parameters)?;
            // 相同领域仅保留最后配置的搜索实体。
            engine_map.insert(engine.domain().to_string(), engine);
        }
        // 未配置通用搜索实体时使用 DuckDuckGo 默认实体兜底。
        let duckduckgo = self
            .get("duckduckgo")
            .expect("DuckDuckGo must be registered");
        engine_map
            .entry(duckduckgo.domain().to_string())
            .or_insert(duckduckgo);
        Ok(engine_map)
    }
}

/// 聚合多个网络搜索引擎的 Agent 工具。
#[derive(Debug)]
pub struct SearchTool {
    client: Client,
    /// 最近一次成功构建时使用的配置。
    configs: RwLock<Value>,
    /// 按领域保存当前可用的搜索实体。
    engines: RwLock<HashMap<String, Arc<dyn SearchEngine>>>,
}

impl SearchTool {
    /// 构建运行时工具参数 Schema。
    fn parameters(&self) -> Value {
        let engines = self
            .engines
            .try_read()
            .expect("SearchTool engines lock must not be held while building parameters");
        let mut domains = engines.keys().collect::<Vec<_>>();
        domains.sort_unstable();
        let properties = serde_json::Map::from_iter([
            (
                "domain".to_string(),
                json!({"type":"string","enum":domains,"description":"Optional search domain. Derive it from the user's request. Omit it when no domain applies."}),
            ),
            (
                "query".to_string(),
                json!({"type":"string","description":"Keywords from the user's request."}),
            ),
        ]);
        json!({"type":"object","properties":properties,"required":["query"],"additionalProperties":false})
    }
}

#[async_trait]
impl AgentTool for SearchTool {
    /// 创建使用默认搜索实体的搜索工具。
    fn new() -> Self {
        Self {
            client: Client::new(),
            configs: RwLock::new(Value::Null),
            engines: RwLock::new(
                SearchRegistry::global()
                    .init(HashMap::new())
                    .expect("DuckDuckGo must be registered"),
            ),
        }
    }

    fn name() -> &'static str {
        "search"
    }

    fn definition(&self) -> Tool {
        Tool {
            name: "search".to_string(),
            description: "Search the web. Results include titles, URLs, and snippets; Do not repeat a search unless its results are insufficient.".to_string(),
            parameters: self.parameters(),
        }
    }

    /// 使用搜索实体配置构建或更新搜索工具。
    /// @param configs 搜索实体名称到参数的 JSON 对象。
    fn init(&self, configs: Value) -> Result<(), AgentToolError> {
        let configs = match configs {
            Value::Null => Value::Object(serde_json::Map::new()),
            Value::Object(_) => configs,
            _ => {
                return Err(AgentToolError::Message(
                    "Search tool configs must be an object".to_string(),
                ));
            }
        };
        let mut current_configs = self
            .configs
            .try_write()
            .expect("SearchTool configs lock must not be held while initializing");
        if *current_configs == configs {
            return Ok(());
        }
        let engines =
            SearchRegistry::global().init(serde_json::from_value(configs.clone()).map_err(
                |error| AgentToolError::Message(format!("Invalid search tool configs: {error}")),
            )?)?;
        *self
            .engines
            .try_write()
            .expect("SearchTool engines lock must not be held while initializing") = engines;
        *current_configs = configs;
        Ok(())
    }

    async fn execute(
        &self,
        _env: &dyn ExecutionEnv,
        _tool_call_id: &String,
        params: &Value,
        _on_update: Option<&UpdateToolCallHook>,
    ) -> Result<AgentToolResult, AgentToolError> {
        let domain = optional_string(params, "domain")?;
        let query = required_string(params, "query")?;
        // 缺省或未命中领域时使用已保证注册的 general 实体。
        let (domain, engine) = {
            let engines = self
                .engines
                .try_read()
                .expect("SearchTool engines lock must not be held while executing");
            let general = "general";
            let (domain, engine) = engines
                .get_key_value(domain.unwrap_or(general))
                .or_else(|| engines.get_key_value(general))
                .expect("SearchTool::new always registers the general search engine");
            (domain.clone(), Arc::clone(engine))
        };
        Ok(AgentToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: engine.search(&self.client, query).await?,
                text_signature: None,
            })],
            details: json!({"domain":domain,"query":query}),
            terminate: None,
        })
    }
}

/// 搜索实体的统一接口。
#[async_trait]
pub trait SearchEngine: std::fmt::Debug + Send + Sync {
    /// 使用默认参数创建搜索实体。
    fn new() -> Self
    where
        Self: Sized;

    /// 搜索实体注册名称。
    fn name() -> &'static str
    where
        Self: Sized;

    /// 搜索实体所属领域。
    fn domain(&self) -> &str;

    /// 返回当前搜索实体参数。
    fn parameters(&self) -> Result<Value, AgentToolError>;

    /// 使用客户端 JSON 参数初始化搜索实体。
    /// @param parameters 搜索实体参数。
    fn init(&self, parameters: Value) -> Result<(), AgentToolError>;

    /// 执行搜索并返回统一的文本结果。
    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError>;
}

/// 读取必填非空字符串。
fn required_string<'a>(params: &'a Value, name: &str) -> Result<&'a str, AgentToolError> {
    params
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AgentToolError::Message(format!("Missing {name}")))
}

/// 读取可选非空字符串，缺省或空白时返回 None。
fn optional_string<'a>(params: &'a Value, name: &str) -> Result<Option<&'a str>, AgentToolError> {
    match params.get(name) {
        None => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(AgentToolError::Message(format!("Invalid {name}"))),
    }
}

/// 将 HTTP 响应解析为 JSON，并转换失败为工具错误。
pub(super) async fn response_json(response: reqwest::Response) -> Result<Value, AgentToolError> {
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        AgentToolError::Message(format!("Search response read failed: {error}"))
    })?;
    if !status.is_success() {
        return Err(AgentToolError::Message(format!(
            "Search request failed ({status}): {body}"
        )));
    }
    serde_json::from_str(&body)
        .map_err(|error| AgentToolError::Message(format!("Invalid search response: {error}")))
}

/// 格式化统一的标题、链接与摘要结果。
pub(super) fn format_results(results: Vec<(String, String, String)>) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }
    results
        .into_iter()
        .enumerate()
        .map(|(index, (title, link, snippet))| {
            format!("{}. {}\n{}\n{}", index + 1, title, link, snippet)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 提取 JSON 字段中的字符串，缺失时返回空字符串。
pub(super) fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
