use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// Tavily 通用搜索实体。
#[derive(Debug)]
pub struct TavilySearch {
    parameters: RwLock<Parameter>,
}

/// Tavily 搜索参数。
#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Parameter {
    /// 服务地址。
    pub url: String,
    /// 服务密钥。
    pub key: String,
    /// 结果数量上限。
    pub limit: usize,
}

#[async_trait]
impl SearchEngine for TavilySearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: "https://api.tavily.com/search".to_string(),
                key: String::new(),
                limit: 10,
            }),
        }
    }
    fn name() -> &'static str {
        "tavily"
    }
    fn domain(&self) -> &str {
        "general"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        serde_json::to_value(
            &*self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("Tavily parameters lock is held".to_string())
            })?,
        )
        .map_err(|error| {
            AgentToolError::Message(format!("Serialize Tavily parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid Tavily parameters: {error}"))
        })?;
        *self
            .parameters
            .try_write()
            .map_err(|_| AgentToolError::Message("Tavily parameters lock is held".to_string()))? =
            parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, key, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("Tavily parameters lock is held".to_string())
            })?;
            (
                parameters.url.clone(),
                parameters.key.clone(),
                parameters.limit,
            )
        };
        let value = response_json(
            client
                .post(url)
                .bearer_auth(key)
                .json(&json!({"query":query,"max_results":limit,"search_depth":"basic"}))
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("Tavily request failed: {error}"))
                })?,
        )
        .await?;
        Ok(format_results(
            value
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|item| {
                    (
                        text(item, "title"),
                        text(item, "url"),
                        text(item, "content"),
                    )
                })
                .collect(),
        ))
    }
}
