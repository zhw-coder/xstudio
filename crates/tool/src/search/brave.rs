use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// Brave 通用搜索实体。
#[derive(Debug)]
pub struct BraveSearch {
    parameters: RwLock<Parameter>,
}

/// Brave 搜索参数。
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
impl SearchEngine for BraveSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: "https://api.search.brave.com/res/v1/web/search".to_string(),
                key: String::new(),
                limit: 10,
            }),
        }
    }

    fn name() -> &'static str {
        "brave"
    }
    fn domain(&self) -> &str {
        "general"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        let parameters = self
            .parameters
            .try_read()
            .map_err(|_| AgentToolError::Message("Brave parameters lock is held".to_string()))?;
        serde_json::to_value(&*parameters).map_err(|error| {
            AgentToolError::Message(format!("Serialize Brave parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid Brave parameters: {error}"))
        })?;
        *self
            .parameters
            .try_write()
            .map_err(|_| AgentToolError::Message("Brave parameters lock is held".to_string()))? =
            parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, key, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("Brave parameters lock is held".to_string())
            })?;
            (
                parameters.url.clone(),
                parameters.key.clone(),
                parameters.limit.to_string(),
            )
        };
        let value = response_json(
            client
                .get(url)
                .header("X-Subscription-Token", key)
                .query(&[("q", query), ("count", limit.as_str())])
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("Brave request failed: {error}"))
                })?,
        )
        .await?;
        Ok(format_results(
            value
                .pointer("/web/results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|item| {
                    (
                        text(item, "title"),
                        text(item, "url"),
                        text(item, "description"),
                    )
                })
                .collect(),
        ))
    }
}
