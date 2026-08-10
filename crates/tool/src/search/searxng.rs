use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// SearXNG 通用搜索实体。
#[derive(Debug)]
pub struct SearxngSearch {
    parameters: RwLock<Parameter>,
}

/// SearXNG 默认返回结果数量。
const DEFAULT_LIMIT: usize = 10;

/// SearXNG 默认服务地址。
const DEFAULT_URL: &str = "https://127.0.0.1:8080";

/// SearXNG 搜索参数。
#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Parameter {
    /// 服务地址列表。
    pub urls: Vec<String>,
    /// 结果数量上限。
    pub limit: usize,
}

#[async_trait]
impl SearchEngine for SearxngSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                urls: vec![DEFAULT_URL.to_string()],
                limit: DEFAULT_LIMIT,
            }),
        }
    }
    fn name() -> &'static str {
        "searxng"
    }
    fn domain(&self) -> &str {
        "general"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        serde_json::to_value(
            &*self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("SearXNG parameters lock is held".to_string())
            })?,
        )
        .map_err(|error| {
            AgentToolError::Message(format!("Serialize SearXNG parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid SearXNG parameters: {error}"))
        })?;
        *self.parameters.try_write().map_err(|_| {
            AgentToolError::Message("SearXNG parameters lock is held".to_string())
        })? = parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (urls, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("SearXNG parameters lock is held".to_string())
            })?;
            (parameters.urls.clone(), parameters.limit)
        };
        let mut last_error = None;
        for url in &urls {
            let result = match client
                .get(url)
                .query(&[("q", query), ("format", "json"), ("pageno", "1")])
                .send()
                .await
            {
                Ok(response) => response_json(response).await,
                Err(error) => Err(AgentToolError::Message(format!(
                    "SearXNG request failed ({url}): {error}"
                ))),
            };
            match result {
                Ok(value) => {
                    return Ok(format_results(
                        value
                            .get("results")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .take(limit)
                            .map(|item| {
                                (
                                    text(item, "title"),
                                    text(item, "url"),
                                    text(item, "content"),
                                )
                            })
                            .collect(),
                    ));
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            AgentToolError::Message("SearXNG requires at least one URL".to_string())
        }))
    }
}
