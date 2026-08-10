use std::{ffi::c_void, slice, str};

use serde_json::{json, Value};
use xstudio_plugin_api::{
    entry::xstudio_plugin_entry_v1, HostApiV1, JsonBytes, PluginDescriptorV1, PluginJsonBytes,
    PluginStatus, PLUGIN_ABI_VERSION,
};

/// 忽略示例插件发出的宿主诊断事件。
unsafe extern "C" fn discard_log(_context: *mut c_void, _event: JsonBytes) {}

/// 模拟宿主默认 Env 的 `exists` 响应。
unsafe extern "C" fn example_env_call(
    _context: *mut c_void,
    request: JsonBytes,
    output: *mut PluginJsonBytes,
) -> PluginStatus {
    let request = unsafe { slice::from_raw_parts(request.data, request.len) };
    let request = str::from_utf8(request).expect("env request must be UTF-8");
    assert!(request.contains("\"operation\":\"exists\""));
    let response = std::mem::ManuallyDrop::new(b"true".to_vec());
    unsafe {
        *output = PluginJsonBytes {
            data: response.as_ptr(),
            len: response.len(),
            free: Some(free_test_json),
        };
    }
    PluginStatus::Ok
}

/// 释放测试宿主分配的 JSON 响应。
unsafe extern "C" fn free_test_json(data: *const u8, len: usize) {
    if !data.is_null() {
        unsafe { drop(Vec::from_raw_parts(data.cast_mut(), len, len)) };
    }
}

/// 创建并初始化示例插件描述符。
fn initialize_plugin() -> PluginDescriptorV1 {
    let host = HostApiV1 {
        abi_version: PLUGIN_ABI_VERSION,
        context: std::ptr::null_mut(),
        log: discard_log,
        env_call: example_env_call,
    };
    let mut descriptor = PluginDescriptorV1 {
        abi_version: 0,
        plugin_context: std::ptr::null_mut(),
        manifest: PluginJsonBytes {
            data: std::ptr::null(),
            len: 0,
            free: None,
        },
        call: missing_call,
    };
    assert_eq!(
        unsafe { xstudio_plugin_entry_v1(&host, &mut descriptor) },
        PluginStatus::Ok
    );
    descriptor
}

/// 读取并释放插件拥有的 JSON 响应。
///
/// @param bytes 插件返回的 JSON 缓冲区。
fn take_json(bytes: PluginJsonBytes) -> Value {
    let result = unsafe { slice::from_raw_parts(bytes.data, bytes.len) };
    let value = serde_json::from_slice(result).expect("example response must be JSON");
    if let Some(free) = bytes.free {
        unsafe { free(bytes.data, bytes.len) };
    }
    value
}

/// 调用插件并取得已释放的 JSON 响应。
///
/// @param descriptor 已初始化的插件描述符。
/// @param request 宿主请求 JSON。
fn call_plugin(descriptor: &PluginDescriptorV1, request: Value) -> (PluginStatus, Value) {
    let request = serde_json::to_vec(&request).expect("test request must serialize");
    let mut output = PluginJsonBytes {
        data: std::ptr::null(),
        len: 0,
        free: None,
    };
    let status = unsafe {
        (descriptor.call)(
            descriptor.plugin_context,
            JsonBytes {
                data: request.as_ptr(),
                len: request.len(),
            },
            &mut output,
        )
    };
    (status, take_json(output))
}

/// 验证入口生成 ABI v1 描述符和可解析清单。
#[test]
fn entry_returns_manifest() {
    let descriptor = initialize_plugin();

    assert_eq!(descriptor.abi_version, PLUGIN_ABI_VERSION);
    assert_eq!(
        take_json(descriptor.manifest)["id"],
        "com.example.xstudio-plugin"
    );
}

// /// 验证示例 Env 请求能够经统一回调返回 JSON。
// ///
// /// 取消 `manifest.rs` 和 `dispatch.rs` 中 Env 相关代码的注释后，才能启用本测试。
// #[test]
// fn call_handles_env_request() {
//     let descriptor = initialize_plugin();
//     if let Some(free) = descriptor.manifest.free {
//         unsafe { free(descriptor.manifest.data, descriptor.manifest.len) };
//     }
//
//     let (status, response) = call_plugin(
//         &descriptor,
//         json!({
//             "kind": "env",
//             "operation": "exists",
//             "cwd": "/project",
//             "arguments": { "path": "README.md" }
//         }),
//     );
//
//     assert_eq!(status, PluginStatus::Ok);
//     assert_eq!(response, json!(false));
// }

/// 验证 Provider 返回可供宿主注册的模型列表。
#[test]
fn provider_returns_models() {
    let descriptor = initialize_plugin();
    if let Some(free) = descriptor.manifest.free {
        unsafe { free(descriptor.manifest.data, descriptor.manifest.len) };
    }

    let (status, response) = call_plugin(
        &descriptor,
        json!({
            "kind": "provider",
            "name": "example-provider",
            "operation": "models",
            "arguments": {
                "provider": "example-provider",
                "baseUrl": "https://example.invalid",
                "options": {},
                "auth": {}
            }
        }),
    );

    assert_eq!(status, PluginStatus::Ok);
    assert_eq!(response[0]["id"], "example-chat");
    assert_eq!(response[0]["provider"], "example-provider");
}

/// 验证 Provider stream 请求返回完整 AssistantMessage JSON。
#[test]
fn provider_stream_returns_assistant_message() {
    let descriptor = initialize_plugin();
    if let Some(free) = descriptor.manifest.free {
        unsafe { free(descriptor.manifest.data, descriptor.manifest.len) };
    }

    let (status, response) = call_plugin(
        &descriptor,
        json!({
            "kind": "provider",
            "name": "example-provider",
            "operation": "stream",
            "arguments": {
                "model": {
                    "id": "example-chat",
                    "api": "example-api",
                    "provider": "example-provider"
                },
                "context": {
                    "messages": [{ "content": "你好，Provider" }]
                },
                "options": {},
                "auth": {}
            }
        }),
    );

    assert_eq!(status, PluginStatus::Ok);
    assert_eq!(response["content"][0]["text"], "你好，Provider");
    assert_eq!(response["stopReason"], "stop");
}

/// 验证 Harness hook 可调用宿主默认 Env JSON 回调。
#[test]
fn harness_hook_calls_host_env() {
    let descriptor = initialize_plugin();
    if let Some(free) = descriptor.manifest.free {
        unsafe { free(descriptor.manifest.data, descriptor.manifest.len) };
    }

    let (status, response) = call_plugin(
        &descriptor,
        json!({
            "kind": "harness",
            "operation": "hook",
            "name": "exampleEnv",
            "arguments": { "cwd": "/project", "event": {} }
        }),
    );

    assert_eq!(status, PluginStatus::Ok);
    assert_eq!(response, json!(true));
}

/// 验证 Tool execute 请求返回 AgentToolResult 兼容 JSON。
#[test]
fn call_executes_tool_request() {
    let descriptor = initialize_plugin();
    if let Some(free) = descriptor.manifest.free {
        unsafe { free(descriptor.manifest.data, descriptor.manifest.len) };
    }

    let (status, response) = call_plugin(
        &descriptor,
        json!({
            "kind": "tool",
            "name": "echo",
            "operation": "execute",
            "arguments": {
                "cwd": "/project",
                "toolCallId": "call-1",
                "params": { "message": "hello" }
            }
        }),
    );

    assert_eq!(status, PluginStatus::Ok);
    assert_eq!(
        response,
        json!({
            "content": [{ "type": "text", "text": "hello" }],
            "details": {
                "tool": "echo",
                "toolCallId": "call-1",
                "cwd": "/project"
            }
        })
    );
}

/// 防止测试描述符中存在空函数指针。
unsafe extern "C" fn missing_call(
    _plugin_context: *mut c_void,
    _request: JsonBytes,
    _output: *mut PluginJsonBytes,
) -> PluginStatus {
    PluginStatus::Failed
}
