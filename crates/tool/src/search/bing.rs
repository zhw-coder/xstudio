use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// Bing Azure 通用搜索实体。
#[derive(Debug)]
pub struct BingSearch {
    parameters: RwLock<Parameter>,
}

/// Bing Azure Web Search API 默认地址。
const DEFAULT_URL: &str = "https://api.bing.microsoft.com/v7.0/search";

/// Bing 默认返回结果数量。
const DEFAULT_LIMIT: usize = 10;

/// Bing 搜索参数。
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
impl SearchEngine for BingSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: DEFAULT_URL.to_string(),
                key: String::new(),
                limit: DEFAULT_LIMIT,
            }),
        }
    }

    fn name() -> &'static str {
        "bing"
    }
    fn domain(&self) -> &str {
        "general"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        let parameters = self
            .parameters
            .try_read()
            .map_err(|_| AgentToolError::Message("Bing parameters lock is held".to_string()))?;
        serde_json::to_value(&*parameters).map_err(|error| {
            AgentToolError::Message(format!("Serialize Bing parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid Bing parameters: {error}"))
        })?;
        *self
            .parameters
            .try_write()
            .map_err(|_| AgentToolError::Message("Bing parameters lock is held".to_string()))? =
            parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, key, limit) = {
            let parameters = self
                .parameters
                .try_read()
                .map_err(|_| AgentToolError::Message("Bing parameters lock is held".to_string()))?;
            (
                parameters.url.clone(),
                parameters.key.clone(),
                parameters.limit.to_string(),
            )
        };
        let value = response_json(
            client
                .get(url)
                .header("Ocp-Apim-Subscription-Key", key)
                .query(&[("q", query), ("count", limit.as_str())])
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("Bing request failed: {error}"))
                })?,
        )
        .await?;
        Ok(format_results(
            value
                .pointer("/webPages/value")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|item| (text(item, "name"), text(item, "url"), text(item, "snippet")))
                .collect(),
        ))
    }
}
