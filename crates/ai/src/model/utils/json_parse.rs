//! 提供普通 JSON repair 与流式 partial JSON 解析的轻量实现。

use serde::de::DeserializeOwned;
use serde_json::Value;

/// JSON 解析错误。
#[derive(Debug, thiserror::Error)]
pub enum JsonParseError {
    /// serde_json 解析失败。
    #[error("JSON parse error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// 尝试修复常见的不完整 JSON 字符串。
pub fn repair_json(json: &str) -> String {
    let mut repaired = json.trim().to_string();
    if repaired.is_empty() {
        return "{}".to_string();
    }

    let mut in_string = false;
    let mut escaped = false;
    let mut stack: Vec<char> = Vec::new();

    for ch in repaired.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if stack.last().copied() == Some(ch) {
                    stack.pop();
                }
            }
            _ => {}
        }
    }

    if in_string {
        repaired.push('"');
    }
    while matches!(repaired.chars().last(), Some(',') | Some(':')) {
        repaired.pop();
    }
    while let Some(ch) = stack.pop() {
        repaired.push(ch);
    }
    repaired
}

/// 使用修复逻辑解析 JSON。
pub fn parse_json_with_repair<T: DeserializeOwned>(json: &str) -> Result<T, JsonParseError> {
    match serde_json::from_str(json) {
        Ok(value) => Ok(value),
        Err(_) => Ok(serde_json::from_str(&repair_json(json))?),
    }
}

/// 解析流式 partial JSON。
pub fn parse_streaming_json<T: DeserializeOwned>(partial_json: Option<&str>) -> Result<T, JsonParseError> {
    parse_json_with_repair(partial_json.unwrap_or("{}"))
}

/// 解析流式 partial JSON 为 `serde_json::Value`。
pub fn parse_streaming_json_value(partial_json: Option<&str>) -> Value {
    parse_streaming_json(partial_json).unwrap_or_else(|_| Value::Object(Default::default()))
}
