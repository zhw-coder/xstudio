use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// GitHub 代码仓库搜索实体。
#[derive(Debug)]
pub struct GitHubSearch {
    parameters: RwLock<Parameter>,
}

/// GitHub 搜索参数。
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
impl SearchEngine for GitHubSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: "https://api.github.com/search/repositories".to_string(),
                key: String::new(),
                limit: 10,
            }),
        }
    }
    fn name() -> &'static str {
        "github"
    }
    fn domain(&self) -> &str {
        "code-repositories"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        serde_json::to_value(
            &*self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("GitHub parameters lock is held".to_string())
            })?,
        )
        .map_err(|error| {
            AgentToolError::Message(format!("Serialize GitHub parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid GitHub parameters: {error}"))
        })?;
        *self
            .parameters
            .try_write()
            .map_err(|_| AgentToolError::Message("GitHub parameters lock is held".to_string()))? =
            parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, key, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("GitHub parameters lock is held".to_string())
            })?;
            (
                parameters.url.clone(),
                parameters.key.clone(),
                parameters.limit.to_string(),
            )
        };
        let mut request = client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "xstudio")
            .query(&[("q", query), ("per_page", limit.as_str())]);
        if !key.is_empty() {
            request = request.bearer_auth(key);
        }
        let value =
            response_json(request.send().await.map_err(|error| {
                AgentToolError::Message(format!("GitHub request failed: {error}"))
            })?)
            .await?;
        Ok(format_results(
            value
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|item| {
                    (
                        text(item, "full_name"),
                        text(item, "html_url"),
                        text(item, "description"),
                    )
                })
                .collect(),
        ))
    }
}
