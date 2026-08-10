//! 该模块保存 Provider / 运行时失败时可挂载到 assistant 消息上的脱敏诊断信息。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 错误诊断的结构化摘要。
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticErrorInfo {
    /// 错误名称或分类。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 人类可读错误消息。
    pub message: String,
    /// 可选错误码。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// 可选 HTTP 状态码。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// 已脱敏的额外错误信息。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// Assistant 消息上携带的诊断记录。
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessageDiagnostic {
    /// 诊断来源，如 provider 名或运行时组件名。
    pub source: String,
    /// 诊断种类。
    pub kind: String,
    /// 诊断发生时间，Unix 毫秒。
    pub timestamp: i64,
    /// 结构化错误摘要。
    pub error: DiagnosticErrorInfo,
}

/// 把任意可调试值格式化为错误字符串。
pub fn format_thrown_value(value: &dyn std::fmt::Debug) -> String {
    format!("{value:?}")
}

/// 从标准错误对象提取诊断错误信息。
pub fn extract_diagnostic_error(error: &dyn std::error::Error) -> DiagnosticErrorInfo {
    DiagnosticErrorInfo {
        name: Some(std::any::type_name_of_val(error).to_string()),
        message: error.to_string(),
        code: None,
        status: None,
        details: None,
    }
}

/// 创建一条 assistant 诊断记录。
pub fn create_assistant_message_diagnostic(
    source: String,
    kind: String,
    error: DiagnosticErrorInfo,
) -> AssistantMessageDiagnostic {
    AssistantMessageDiagnostic { source, kind, timestamp: crate::model::types::now_millis(), error }
}

/// 向诊断数组追加一条记录。
pub fn append_assistant_message_diagnostic(
    diagnostics: &mut Vec<AssistantMessageDiagnostic>,
    diagnostic: AssistantMessageDiagnostic,
) {
    diagnostics.push(diagnostic);
}
