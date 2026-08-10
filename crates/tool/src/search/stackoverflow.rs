use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// Stack Overflow 编程问题搜索实体。
#[derive(Debug)]
pub struct StackOverflowSearch {
    parameters: RwLock<Parameter>,
}

/// Stack Overflow 搜索参数。
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
impl SearchEngine for StackOverflowSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: "https://api.stackexchange.com/2.3/search/advanced".to_string(),
                key: String::new(),
                limit: 10,
            }),
        }
    }
    fn name() -> &'static str {
        "stackoverflow"
    }
    fn domain(&self) -> &str {
        "programming-questions"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        serde_json::to_value(&*self.parameters.try_read().map_err(|_| {
            AgentToolError::Message("Stack Overflow parameters lock is held".to_string())
        })?)
        .map_err(|error| {
            AgentToolError::Message(format!(
                "Serialize Stack Overflow parameters failed: {error}"
            ))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid Stack Overflow parameters: {error}"))
        })?;
        *self.parameters.try_write().map_err(|_| {
            AgentToolError::Message("Stack Overflow parameters lock is held".to_string())
        })? = parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        // Stack Exchange 接口要求字符串形式的结果上限。
        let (url, key, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("Stack Overflow parameters lock is held".to_string())
            })?;
            (
                parameters.url.clone(),
                parameters.key.clone(),
                parameters.limit.to_string(),
            )
        };
        let mut query_params = vec![
            ("site", "stackoverflow"),
            ("q", query),
            ("pagesize", limit.as_str()),
            ("filter", "withbody"),
        ];
        if !key.is_empty() {
            query_params.push(("key", &key));
        }
        let value = response_json(client.get(url).query(&query_params).send().await.map_err(
            |error| AgentToolError::Message(format!("Stack Overflow request failed: {error}")),
        )?)
        .await?;
        Ok(format_results(
            value
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|item| (text(item, "title"), text(item, "link"), text(item, "body")))
                .collect(),
        ))
    }
}
