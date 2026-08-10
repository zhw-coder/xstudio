use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// Crossref 学术出版物搜索实体。
#[derive(Debug)]
pub struct CrossrefSearch {
    parameters: RwLock<Parameter>,
}

/// Crossref 搜索参数。
#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Parameter {
    /// 服务地址。
    pub url: String,
    /// 结果数量上限。
    pub limit: usize,
}

#[async_trait]
impl SearchEngine for CrossrefSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: "https://api.crossref.org/works".to_string(),
                limit: 10,
            }),
        }
    }
    fn name() -> &'static str {
        "crossref"
    }
    fn domain(&self) -> &str {
        "academic-publications"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        serde_json::to_value(
            &*self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("Crossref parameters lock is held".to_string())
            })?,
        )
        .map_err(|error| {
            AgentToolError::Message(format!("Serialize Crossref parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid Crossref parameters: {error}"))
        })?;
        *self.parameters.try_write().map_err(|_| {
            AgentToolError::Message("Crossref parameters lock is held".to_string())
        })? = parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("Crossref parameters lock is held".to_string())
            })?;
            (parameters.url.clone(), parameters.limit.to_string())
        };
        let value = response_json(
            client
                .get(url)
                .query(&[("query", query), ("rows", limit.as_str())])
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("Crossref request failed: {error}"))
                })?,
        )
        .await?;
        Ok(format_results(
            value
                .pointer("/message/items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|item| {
                    let title = item
                        .get("title")
                        .and_then(Value::as_array)
                        .and_then(|items| items.first())
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    (
                        title,
                        item.get("URL")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        text(item, "publisher"),
                    )
                })
                .collect(),
        ))
    }
}
