use ai::agent::AgentToolError;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::RwLock;

use super::{format_results, response_json, text};
use crate::search::SearchEngine;

/// PubMed 生物医学搜索实体。
#[derive(Debug)]
pub struct PubMedSearch {
    parameters: RwLock<Parameter>,
}

/// PubMed 搜索参数。
#[derive(Debug, Deserialize, serde::Serialize)]
pub struct Parameter {
    /// 服务地址。
    pub url: String,
    /// 结果数量上限。
    pub limit: usize,
}

#[async_trait]
impl SearchEngine for PubMedSearch {
    fn new() -> Self {
        Self {
            parameters: RwLock::new(Parameter {
                url: "https://eutils.ncbi.nlm.nih.gov/entrez/eutils".to_string(),
                limit: 10,
            }),
        }
    }
    fn name() -> &'static str {
        "pubmed"
    }
    fn domain(&self) -> &str {
        "biomedical"
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        serde_json::to_value(
            &*self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("PubMed parameters lock is held".to_string())
            })?,
        )
        .map_err(|error| {
            AgentToolError::Message(format!("Serialize PubMed parameters failed: {error}"))
        })
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        let parameters = serde_json::from_value(parameters).map_err(|error| {
            AgentToolError::Message(format!("Invalid PubMed parameters: {error}"))
        })?;
        *self
            .parameters
            .try_write()
            .map_err(|_| AgentToolError::Message("PubMed parameters lock is held".to_string()))? =
            parameters;
        Ok(())
    }

    async fn search(&self, client: &Client, query: &str) -> Result<String, AgentToolError> {
        let (base, limit) = {
            let parameters = self.parameters.try_read().map_err(|_| {
                AgentToolError::Message("PubMed parameters lock is held".to_string())
            })?;
            (
                parameters.url.trim_end_matches('/').to_string(),
                parameters.limit.to_string(),
            )
        };
        let search = response_json(
            client
                .get(format!("{base}/esearch.fcgi"))
                .query(&[
                    ("db", "pubmed"),
                    ("term", query),
                    ("retmode", "json"),
                    ("retmax", limit.as_str()),
                ])
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("PubMed request failed: {error}"))
                })?,
        )
        .await?;
        let ids = search
            .pointer("/esearchresult/idlist")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentToolError::Message("Invalid PubMed search response".to_string()))?
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",");
        if ids.is_empty() {
            return Ok("No results found.".to_string());
        }
        let summary = response_json(
            client
                .get(format!("{base}/esummary.fcgi"))
                .query(&[("db", "pubmed"), ("id", &ids), ("retmode", "json")])
                .send()
                .await
                .map_err(|error| {
                    AgentToolError::Message(format!("PubMed summary request failed: {error}"))
                })?,
        )
        .await?;
        Ok(format_results(
            ids.split(',')
                .filter_map(|id| summary.pointer(&format!("/result/{id}")))
                .map(|item| {
                    (
                        text(item, "title"),
                        format!("https://pubmed.ncbi.nlm.nih.gov/{}/", text(item, "uid")),
                        text(item, "sortpubdate"),
                    )
                })
                .collect(),
        ))
    }
}
