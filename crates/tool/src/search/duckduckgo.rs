use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// DuckDuckGo 通用搜索实体。
#[derive(Debug)]
pub struct DuckDuckGoSearch {
    parameters: RwLock<Parameter>,
}

/// DuckDuckGo Instant Answer API 默认地址。
const DEFAULT_URL: &str = "https://api.duckduckgo.com/";

/// DuckDuckGo 默认返回结果数量。
const DEFAULT_LIMIT: usize = 10;

/// DuckDuckGo 搜索参数。
#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Parameter {
    /// 服务地址。
    pub url: String,
    /// 结果数量上限。
    pub limit: usize,
}

#[async_trait]
impl SearchEngine for DuckDuckGoSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: DEFAULT_URL.to_string(),
                limit: DEFAULT_LIMIT,
            }),
        }
    }

    fn name() -> &'static str {
        "duckduckgo"
    }
    fn domain(&self) -> &str {
        "general"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        let parameters = self.parameters.try_read().map_err(|_| {
            AgentToolError::Message("DuckDuckGo parameters lock is held".to_string())
        })?;
        serde_json::to_value(&*parameters).map_err(|error| {
            AgentToolError::Message(format!("Serialize DuckDuckGo parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid DuckDuckGo parameters: {error}"))
        })?;
        *self.parameters.try_write().map_err(|_| {
            AgentToolError::Message("DuckDuckGo parameters lock is held".to_string())
        })? = parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("DuckDuckGo parameters lock is held".to_string())
            })?;
            (parameters.url.clone(), parameters.limit)
        };
        let value = response_json(
            client
                .get(url)
                .query(&[("q", query), ("format", "json"), ("no_html", "1")])
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("DuckDuckGo request failed: {error}"))
                })?,
        )
        .await?;
        let mut results = Vec::new();
        if let Some(answer) = value
            .get("AbstractText")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            results.push((
                text(&value, "Heading"),
                text(&value, "AbstractURL"),
                answer.to_string(),
            ));
        }
        if let Some(topics) = value.get("RelatedTopics").and_then(Value::as_array) {
            collect_topics(topics, &mut results, limit);
        }
        Ok(format_results(results))
    }
}

/// 递归收集 DuckDuckGo 的嵌套主题。
///
/// # 参数
/// - `topics`: 当前层级主题。
/// - `results`: 累积的搜索结果。
/// - `maximum`: 结果数量上限。
fn collect_topics(topics: &[Value], results: &mut Vec<(String, String, String)>, maximum: usize) {
    for topic in topics {
        if results.len() >= maximum {
            break;
        }
        if let Some(nested) = topic.get("Topics").and_then(Value::as_array) {
            collect_topics(nested, results, maximum);
        } else if let Some(snippet) = topic.get("Text").and_then(Value::as_str) {
            results.push((
                snippet.to_string(),
                text(topic, "FirstURL"),
                snippet.to_string(),
            ));
        }
    }
}
