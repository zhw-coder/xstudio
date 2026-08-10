/// 默认工具输出行数上限。
pub const DEFAULT_MAX_LINES: usize = 2_000;
/// 默认工具输出 UTF-8 字节上限。
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024;
/// grep 单行输出字符上限。
pub const GREP_MAX_LINE_LENGTH: usize = 500;

/// 工具输出截断维度。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TruncatedBy {
    /// 行数达到上限。
    Lines,
    /// 字节数达到上限。
    Bytes,
}

/// 截断后的输出及其统计信息。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TruncationResult {
    /// 截断后的文本。
    pub content: String,
    /// 是否发生截断。
    pub truncated: bool,
    /// 触发截断的维度。
    pub truncated_by: Option<TruncatedBy>,
    /// 原始行数。
    pub total_lines: usize,
    /// 原始 UTF-8 字节数。
    pub total_bytes: usize,
    /// 输出行数。
    pub output_lines: usize,
    /// 输出 UTF-8 字节数。
    pub output_bytes: usize,
    /// 首行是否超过字节上限。
    pub first_line_exceeds_limit: bool,
    /// 实际行数上限。
    pub max_lines: usize,
    /// 实际字节上限。
    pub max_bytes: usize,
}

/// 将字节数格式化为人类可读字符串。
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// 从文本开头保留完整行，并同时遵守行数与字节上限。
pub fn truncate_head(content: &str, max_lines: usize, max_bytes: usize) -> TruncationResult {
    let lines: Vec<&str> = content.split('\n').collect();
    let total_lines = lines.len();
    let total_bytes = content.len();
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return TruncationResult {
            content: content.to_string(),
            truncated: false,
            truncated_by: None,
            total_lines,
            total_bytes,
            output_lines: total_lines,
            output_bytes: total_bytes,
            first_line_exceeds_limit: false,
            max_lines,
            max_bytes,
        };
    }

    if lines.first().is_some_and(|line| line.len() > max_bytes) {
        return TruncationResult {
            content: String::new(),
            truncated: true,
            truncated_by: Some(TruncatedBy::Bytes),
            total_lines,
            total_bytes,
            output_lines: 0,
            output_bytes: 0,
            first_line_exceeds_limit: true,
            max_lines,
            max_bytes,
        };
    }

    let mut selected = Vec::new();
    let mut bytes = 0;
    let mut truncated_by = TruncatedBy::Lines;
    for (index, line) in lines.iter().enumerate() {
        if index >= max_lines {
            break;
        }
        let line_bytes = line.len() + usize::from(!selected.is_empty());
        if bytes + line_bytes > max_bytes {
            truncated_by = TruncatedBy::Bytes;
            break;
        }
        selected.push(*line);
        bytes += line_bytes;
    }

    let output = selected.join("\n");
    TruncationResult {
        output_bytes: output.len(),
        output_lines: selected.len(),
        content: output,
        truncated: true,
        truncated_by: Some(truncated_by),
        total_lines,
        total_bytes,
        first_line_exceeds_limit: false,
        max_lines,
        max_bytes,
    }
}

/// 截断单行 grep 输出。
pub fn truncate_line(line: &str) -> (String, bool) {
    let mut characters = line.chars();
    let prefix: String = characters.by_ref().take(GREP_MAX_LINE_LENGTH).collect();
    if characters.next().is_some() {
        (format!("{prefix}... [truncated]"), true)
    } else {
        (prefix, false)
    }
}
