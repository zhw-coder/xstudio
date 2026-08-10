//! 统一 JSON 调用分发。
//!
//! 每种能力按 `kind`、`name` 与 `operation` 分发。空实现用于验证插件装配；开发时
//! 应替换为真实业务逻辑并仅保留清单中声明的能力。

use std::{ffi::c_void, slice};

use serde_json::{json, Value};

use crate::{JsonBytes, PluginJsonBytes, PluginStatus};

/// 宿主默认 ExecutionEnv 的同步 JSON 客户端。
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct HostEnvApi {
    /// 宿主回调上下文。
    context: *mut c_void,
    /// 宿主 Env JSON 回调。
    call: unsafe extern "C" fn(*mut c_void, JsonBytes, *mut PluginJsonBytes) -> PluginStatus,
}

impl HostEnvApi {
    /// 创建宿主 Env 客户端。
    ///
    /// @param context 宿主回调上下文。
    /// @param call 宿主 Env JSON 回调。
    pub fn new(
        context: *mut c_void,
        call: unsafe extern "C" fn(*mut c_void, JsonBytes, *mut PluginJsonBytes) -> PluginStatus,
    ) -> Self {
        Self { context, call }
    }

    /// 调用宿主默认 Env。
    ///
    /// @param request 含 `cwd`、`operation` 与 `arguments` 的 JSON 请求。
    pub fn call(&self, request: Value) -> Result<Value, (PluginStatus, String)> {
        let request = serde_json::to_vec(&request)
            .map_err(|error| (PluginStatus::Failed, error.to_string()))?;
        let mut output = PluginJsonBytes {
            data: std::ptr::null(),
            len: 0,
            free: None,
        };
        let status = unsafe {
            (self.call)(
                self.context,
                JsonBytes {
                    data: request.as_ptr(),
                    len: request.len(),
                },
                &mut output,
            )
        };
        if status != PluginStatus::Ok {
            return Err((status, "宿主 Env 调用失败".to_string()));
        }
        let response = if output.len > 0 && output.data.is_null() {
            Err((PluginStatus::Failed, "宿主 Env 返回空指针".to_string()))
        } else {
            let bytes = unsafe { slice::from_raw_parts(output.data, output.len) };
            serde_json::from_slice(bytes).map_err(|error| (PluginStatus::Failed, error.to_string()))
        };
        if let Some(free) = output.free {
            unsafe { free(output.data, output.len) };
        }
        response
    }
}

/// 分发插件调用请求。
///
/// @param request 宿主传入的完整 JSON 请求。
/// @returns 成功时的 JSON 响应，失败时的 ABI 状态码与诊断文本。
pub fn dispatch(request: Value, host_env: HostEnvApi) -> Result<Value, (PluginStatus, String)> {
    let kind = required_string(&request, "kind")?;
    let operation = required_string(&request, "operation")?;

    match kind {
        // "env" => handle_env(operation, &request),
        "provider" => handle_provider(operation, &request),
        "tool" => handle_tool(operation, &request),
        "search" => handle_search(operation),
        "harness" => handle_harness(operation, &request, &host_env),
        _ => Err((PluginStatus::NotSupported, format!("不支持的 kind: {kind}"))),
    }
}

// /// 处理 ExecutionEnv 请求。
// ///
// /// 取消 `manifest.rs` 中的 `env` capability 注释，并恢复上方 `env` 分支后才会调用。
// /// 默认保持注释，以便宿主使用 `LocalExecutionEnv`。
// fn handle_env(operation: &str, request: &Value) -> Result<Value, (PluginStatus, String)> {
//     let _cwd = required_string(request, "cwd")?;
//     match operation {
//         "exists" => Ok(json!(false)),
//         "read_text_file" => Ok(json!("")),
//         _ => unsupported("env", operation),
//     }
// }

/// 处理 Provider 请求。
///
/// v1 的 `stream` 与 `streamSimple` 必须返回完整 `AssistantMessage` JSON。案例 Provider
/// 返回固定模型和回显最后一条用户文本的完整回复；复制后替换为实际服务调用即可。
fn handle_provider(operation: &str, request: &Value) -> Result<Value, (PluginStatus, String)> {
    match operation {
        "models" => provider_models(request),
        "stream" | "streamSimple" => provider_message(request),
        _ => unsupported("provider", operation),
    }
}

/// 返回示例 Provider 的模型元数据。
///
/// @param request 宿主传入的 Provider 请求。
fn provider_models(request: &Value) -> Result<Value, (PluginStatus, String)> {
    let arguments = required_object(request, "arguments")?;
    let provider = required_string_value(arguments, "provider")?;
    let base_url = required_string_value(arguments, "baseUrl")?;

    Ok(json!([{
        "id": "example-chat",
        "name": "Example Chat",
        "api": "example-api",
        "provider": provider,
        "baseUrl": base_url,
        "reasoning": false,
        "input": ["text"],
        "cost": {},
        "contextWindow": 16384,
        "maxTokens": 4096
    }]))
}

/// 返回完整的示例助手消息。
///
/// @param request 宿主传入的流式 Provider 请求。
fn provider_message(request: &Value) -> Result<Value, (PluginStatus, String)> {
    let arguments = required_object(request, "arguments")?;
    let model = required_object_value(arguments, "model")?;
    let context = required_object_value(arguments, "context")?;
    let model_id = required_string_value(model, "id")?;
    let api = required_string_value(model, "api")?;
    let provider = required_string_value(model, "provider")?;
    let text = latest_user_text(context).unwrap_or("你好，我是示例 Provider。");

    Ok(json!({
        "content": [{ "type": "text", "text": text }],
        "api": api,
        "provider": provider,
        "model": model_id,
        "usage": {},
        "stopReason": "stop",
        "timestamp": 0
    }))
}

/// 读取上下文中最后一个用户文本块。
///
/// @param context 宿主序列化的 `Context` JSON。
fn latest_user_text(context: &serde_json::Map<String, Value>) -> Option<&str> {
    context
        .get("messages")?
        .as_array()?
        .iter()
        .rev()
        .find_map(|message| {
            let content = message.get("content")?;
            content.as_str().or_else(|| {
                content.as_array()?.iter().rev().find_map(|block| {
                    (block.get("type")?.as_str() == Some("text"))
                        .then(|| block.get("text")?.as_str())
                        .flatten()
                })
            })
        })
}

/// 处理 AgentTool 请求。
fn handle_tool(operation: &str, request: &Value) -> Result<Value, (PluginStatus, String)> {
    match operation {
        "init" => Ok(Value::Null),
        "execute" => execute_tool(request),
        _ => unsupported("tool", operation),
    }
}

/// 执行插件清单中声明的工具。
///
/// 此案例不按固定工具名称分发，所有工具均读取 `params.message` 并原样返回。复制后可
/// 使用 `name` 字段继续按多个工具名称分发。
fn execute_tool(request: &Value) -> Result<Value, (PluginStatus, String)> {
    let name = required_string(request, "name")?;
    let arguments = request
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_argument("arguments 必须是对象"))?;
    let cwd = arguments
        .get("cwd")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_argument("arguments.cwd 必须是字符串"))?;
    let tool_call_id = arguments
        .get("toolCallId")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_argument("arguments.toolCallId 必须是字符串"))?;
    let message = arguments
        .get("params")
        .and_then(Value::as_object)
        .and_then(|params| params.get("message"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_argument("arguments.params.message 必须是字符串"))?;

    Ok(json!({
        "content": [{
            "type": "text",
            "text": message
        }],
        "details": {
            "tool": name,
            "toolCallId": tool_call_id,
            "cwd": cwd
        }
    }))
}

/// 处理 SearchEngine 请求。
fn handle_search(operation: &str) -> Result<Value, (PluginStatus, String)> {
    match operation {
        "parameters" => Ok(json!({})),
        "init" => Ok(Value::Null),
        "search" => Ok(json!("")),
        _ => unsupported("search", operation),
    }
}

/// 处理 Harness 事件与控制型 hook。
///
/// `null` 表示不修改宿主事件处理结果。实现 hook 时可通过 `host_env` 调用宿主默认
/// ExecutionEnv，例如 `host_env.call(json!({"cwd":"/project","operation":"exists","arguments":{"path":"README.md"}}))`。
fn handle_harness(
    operation: &str,
    request: &Value,
    host_env: &HostEnvApi,
) -> Result<Value, (PluginStatus, String)> {
    match operation {
        "hook" if request.get("name").and_then(Value::as_str) == Some("exampleEnv") => host_env
            .call(json!({
                "cwd": required_string_value(required_object(request, "arguments")?, "cwd")?,
                "operation": "exists",
                "arguments": { "path": "README.md" }
            })),
        "event" | "hook" => Ok(Value::Null),
        _ => unsupported("harness", operation),
    }
}

/// 构建未实现操作错误。
fn unsupported(kind: &str, operation: &str) -> Result<Value, (PluginStatus, String)> {
    Err((
        PluginStatus::NotSupported,
        format!("{kind} operation 尚未实现: {operation}"),
    ))
}

/// 构建参数格式错误。
fn invalid_argument(message: &str) -> (PluginStatus, String) {
    (PluginStatus::InvalidArgument, message.to_string())
}

/// 读取请求中的必填对象字段。
///
/// @param request JSON 对象。
/// @param field 字段名称。
fn required_object<'a>(
    request: &'a Value,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>, (PluginStatus, String)> {
    request
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_argument(&format!("{field} 必须是对象")))
}

/// 读取 JSON 对象中的必填字符串字段。
///
/// @param request JSON 对象。
/// @param field 字段名称。
fn required_string_value<'a>(
    request: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<&'a str, (PluginStatus, String)> {
    request
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_argument(&format!("{field} 必须是字符串")))
}

/// 读取 JSON 对象中的必填对象字段。
///
/// @param request JSON 对象。
/// @param field 字段名称。
fn required_object_value<'a>(
    request: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>, (PluginStatus, String)> {
    request
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_argument(&format!("{field} 必须是对象")))
}

/// 读取请求中的必填字符串字段。
fn required_string<'a>(request: &'a Value, field: &str) -> Result<&'a str, (PluginStatus, String)> {
    request.get(field).and_then(Value::as_str).ok_or_else(|| {
        (
            PluginStatus::InvalidArgument,
            format!("缺少字符串字段: {field}"),
        )
    })
}
