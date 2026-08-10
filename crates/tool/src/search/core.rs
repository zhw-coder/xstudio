use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// CORE 开放论文搜索实体。
#[derive(Debug)]
pub struct CoreSearch {
    parameters: RwLock<Parameter>,
}

/// CORE 搜索参数。
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
impl SearchEngine for CoreSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: "https://api.core.ac.uk/v3/search/works".to_string(),
                key: String::new(),
                limit: 10,
            }),
        }
    }
    fn name() -> &'static str {
        "core"
    }
    fn domain(&self) -> &str {
        "open-papers"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        serde_json::to_value(
            &*self
                .parameters
                .try_read()
                .map_err(|_| AgentToolError::Message("CORE parameters lock is held".to_string()))?,
        )
        .map_err(|error| {
            AgentToolError::Message(format!("Serialize CORE parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid CORE parameters: {error}"))
        })?;
        *self
            .parameters
            .try_write()
            .map_err(|_| AgentToolError::Message("CORE parameters lock is held".to_string()))? =
            parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, key, limit) = {
            let parameters = self
                .parameters
                .try_read()
                .map_err(|_| AgentToolError::Message("CORE parameters lock is held".to_string()))?;
            (
                parameters.url.clone(),
                parameters.key.clone(),
                parameters.limit.to_string(),
            )
        };
        let value = response_json(
            client
                .get(url)
                .header("Authorization", format!("Bearer {key}"))
                .query(&[("q", query), ("limit", limit.as_str())])
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("CORE request failed: {error}"))
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
                        item.pointer("/downloadUrl")
                            .and_then(Value::as_str)
                            .or_else(|| {
                                item.pointer("/sourceFulltextUrls/0")
                                    .and_then(Value::as_str)
                            })
                            .unwrap_or_default()
                            .to_string(),
                        text(item, "abstract"),
                    )
                })
                .collect(),
        ))
    }
}
