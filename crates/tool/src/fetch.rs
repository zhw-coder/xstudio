//! 网页正文抓取 Agent 工具。

use ai::{
    agent::{env::ExecutionEnv, AgentTool, AgentToolError, AgentToolResult, UpdateToolCallHook},
    model::Tool,
};
use async_trait::async_trait;
use futures::future::join_all;
use reqwest::{redirect::Policy, Client};
use scraper::{ElementRef, Html, Selector};
use serde_json::{json, Value};

use crate::{
    lib::truncate::{format_size, truncate_head, DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES},
    read::text_result,
};

/// 抓取 URL 并提取网页正文纯文本的 Agent 工具。
#[derive(Debug)]
pub struct FetchTool {
    client: Client,
}

impl Default for FetchTool {
    /// 使用默认 HTTP 客户端创建网页抓取工具。
    fn default() -> Self {
        <Self as AgentTool>::new()
    }
}

#[async_trait]
impl AgentTool for FetchTool {
    /// 创建支持最多十次重定向的网页抓取工具。
    fn new() -> Self {
        Self {
            client: Client::builder()
                .redirect(Policy::limited(10))
                .user_agent("xstudio-fetch/0.1")
                .build()
                .expect("FetchTool client configuration should be valid"),
        }
    }

    fn name() -> &'static str {
        "fetch"
    }

    fn definition(&self) -> Tool {
        Tool {
            name: "fetch".to_string(),
            description: "Fetch webpage text for URLs. Include all needed URLs in one call."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "urls": {
                        "type": "array",
                        "items": { "type": "string", "minLength": 1 },
                        "description": "URLs to fetch together"
                    }
                },
                "required": ["urls"],
                "additionalProperties": false
            }),
        }
    }

    fn init(&self, _configs: Value) -> Result<(), AgentToolError> {
        Ok(())
    }

    async fn execute(
        &self,
        _env: &dyn ExecutionEnv,
        _tool_call_id: &String,
        params: &Value,
        _on_update: Option<&UpdateToolCallHook>,
    ) -> Result<AgentToolResult, AgentToolError> {
        let urls = required_urls(params)?;
        let mut sections = Vec::with_capacity(urls.len());
        let mut results = Vec::with_capacity(urls.len());
        let fetches = urls
            .iter()
            .map(|url| async { (*url, self.fetch_url(url).await) });
        for (url, result) in join_all(fetches).await {
            match result {
                Ok(result) => {
                    sections.push(format!("## {url}\n{}", result.content));
                    results.push(json!({
                        "url": url,
                        "status": result.status,
                        "contentType": result.content_type,
                        "truncation": result.truncation,
                    }));
                }
                Err(error) => {
                    sections.push(format!("## {url}\n[Fetch failed: {error}]"));
                    results.push(json!({
                        "url": url,
                        "status": 500,
                        "error": error.to_string(),
                    }));
                }
            }
        }
        let output = truncate_output(&sections.join("\n\n"));

        Ok(text_result(
            output.content,
            json!({
                "results": results,
                "truncation": output.truncation,
            }),
        ))
    }
}

impl FetchTool {
    /// 抓取单个 URL，并提取其网页正文。
    /// @param url 待抓取的 HTTP(S) URL。
    async fn fetch_url(&self, url: &str) -> Result<FetchResult, AgentToolError> {
        let parsed_url = reqwest::Url::parse(url)
            .map_err(|error| AgentToolError::Message(format!("Invalid URL: {error}")))?;
        if !matches!(parsed_url.scheme(), "http" | "https") {
            return Err(AgentToolError::Message(
                "URL scheme must be http or https".to_string(),
            ));
        }

        let response =
            self.client.get(parsed_url).send().await.map_err(|error| {
                AgentToolError::Message(format!("Failed to fetch {url}: {error}"))
            })?;
        let status = response.status();
        let final_url = response.url().to_string();
        if !status.is_success() {
            return Err(AgentToolError::Message(format!(
                "Fetch request failed ({status}) for {final_url}"
            )));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response.text().await.map_err(|error| {
            AgentToolError::Message(format!(
                "Failed to read response body from {final_url}: {error}"
            ))
        })?;
        let content = extract_readable_text(&body);
        let truncation = truncate_head(&content, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
        let mut output = truncation.content.clone();
        if truncation.first_line_exceeds_limit {
            output = format!(
                "[The first extracted line exceeds the {} output limit.]",
                format_size(DEFAULT_MAX_BYTES)
            );
        } else if truncation.truncated {
            output.push_str(&format!(
                "\n\n[Webpage text truncated to {}.]",
                format_size(DEFAULT_MAX_BYTES)
            ));
        }
        if output.is_empty() {
            output = "No readable text found.".to_string();
        }

        Ok(FetchResult {
            content: output,
            status: status.as_u16(),
            content_type,
            truncation,
        })
    }
}

/// 单个 URL 的成功抓取结果。
#[derive(Debug)]
struct FetchResult {
    /// 提取并截断后的正文。
    content: String,
    /// HTTP 响应状态码。
    status: u16,
    /// HTTP 响应内容类型。
    content_type: String,
    /// 单个网页正文的截断信息。
    truncation: crate::lib::truncate::TruncationResult,
}

/// 全部 URL 的组合输出。
struct FetchOutput {
    /// 最终展示给模型的文本。
    content: String,
    /// 全部 URL 结果组合后的截断信息。
    truncation: crate::lib::truncate::TruncationResult,
}

/// 读取必填且非空的 urls 字符串数组。
/// @param params 工具调用参数。
fn required_urls(params: &Value) -> Result<Vec<&str>, AgentToolError> {
    let urls = params
        .get("urls")
        .and_then(Value::as_array)
        .filter(|urls| !urls.is_empty())
        .ok_or_else(|| AgentToolError::Message("urls must be a non-empty array".to_string()))?;
    urls.iter()
        .map(|value| {
            value
                .as_str()
                .filter(|url| !url.trim().is_empty())
                .ok_or_else(|| {
                    AgentToolError::Message("urls items must be non-empty strings".to_string())
                })
        })
        .collect()
}

/// 截断多个 URL 的组合输出并追加截断提示。
/// @param content 待输出的全部抓取结果。
fn truncate_output(content: &str) -> FetchOutput {
    let truncation = truncate_head(content, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
    let mut output = truncation.content.clone();
    if truncation.first_line_exceeds_limit {
        output = format!(
            "[The first result line exceeds the {} output limit.]",
            format_size(DEFAULT_MAX_BYTES)
        );
    } else if truncation.truncated {
        output.push_str(&format!(
            "\n\n[Combined fetch output truncated to {}.]",
            format_size(DEFAULT_MAX_BYTES)
        ));
    }
    FetchOutput {
        content: output,
        truncation,
    }
}

/// 从 HTML 中优先提取 article 或 main 区域的可读文本。
/// @param html 原始 HTML 文本。
fn extract_readable_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let article_selector = Selector::parse("article").expect("article selector should be valid");
    let main_selector = Selector::parse("main").expect("main selector should be valid");
    let body_selector = Selector::parse("body").expect("body selector should be valid");
    let root = document
        .select(&article_selector)
        .next()
        .or_else(|| document.select(&main_selector).next())
        .or_else(|| document.select(&body_selector).next());
    root.map_or_else(String::new, collect_text)
}

/// 递归收集元素及其后代中的可读文本节点。
/// @param element 待提取的 HTML 元素。
fn collect_text(element: ElementRef<'_>) -> String {
    let text = element
        .text()
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    normalize_whitespace(&text)
}

/// 规范化连续空白字符，方便模型消费网页正文。
/// @param text 待规范化的文本。
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
