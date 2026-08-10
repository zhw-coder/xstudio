use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// 千帆百度通用搜索实体。
#[derive(Debug)]
pub struct BaiduSearch {
    parameters: RwLock<Parameter>,
}

/// 千帆百度搜索 V2 默认地址。
const DEFAULT_URL: &str = "https://qianfan.baidubce.com/v2/ai_search/web_search";

/// 千帆百度搜索默认返回结果数量。
const DEFAULT_LIMIT: usize = 10;

/// 千帆百度搜索参数。
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
impl SearchEngine for BaiduSearch {
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
        "baidu"
    }
    fn domain(&self) -> &str {
        "general"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        let parameters = self
            .parameters
            .try_read()
            .map_err(|_| AgentToolError::Message("Baidu parameters lock is held".to_string()))?;
        serde_json::to_value(&*parameters).map_err(|error| {
            AgentToolError::Message(format!("Serialize Baidu parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid Baidu parameters: {error}"))
        })?;
        *self
            .parameters
            .try_write()
            .map_err(|_| AgentToolError::Message("Baidu parameters lock is held".to_string()))? =
            parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (url, key, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("Baidu parameters lock is held".to_string())
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
                .header("X-Appbuilder-Authorization", format!("Bearer {key}"))
                .json(&json!({
                    "messages":[{"role":"user","content":query}],
                    "search_source":"baidu_search_v2",
                    "resource_type_filter":[{"type":"web","top_k":limit}],
                }))
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("Baidu request failed: {error}"))
                })?,
        )
        .await?;
        Ok(format_results(
            value
                .get("references")
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
