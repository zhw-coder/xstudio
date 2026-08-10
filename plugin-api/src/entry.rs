//! 示例插件的 `xstudio_plugin_entry_v1` 导出入口。
//!
//! 本目录可直接复制为独立插件项目；保留本文件中的入口符号和 C ABI 类型。

use std::{ffi::c_void, slice};

use serde_json::{json, Value};

use crate::{
    dispatch::{dispatch, HostEnvApi},
    manifest::plugin_manifest,
    response::json_response,
    HostApiV1, JsonBytes, PluginDescriptorV1, PluginJsonBytes, PluginStatus, PLUGIN_ABI_VERSION,
};

/// 插件进程级状态。
///
/// 可在此增加插件自身拥有的配置、客户端或缓存；不能保存宿主请求的临时字节指针。
struct PluginState {
    /// 宿主日志回调。
    host_log: unsafe extern "C" fn(*mut c_void, JsonBytes),
    /// 宿主默认 ExecutionEnv JSON 回调。
    host_env_call:
        unsafe extern "C" fn(*mut c_void, JsonBytes, *mut PluginJsonBytes) -> PluginStatus,
    /// 与日志回调配对的宿主上下文。
    host_context: *mut c_void,
}

/// 导出固定 ABI v1 入口。
///
/// @param host 宿主提供的 ABI v1 函数表。
/// @param output 由插件填写的描述符地址。
#[no_mangle]
pub unsafe extern "C" fn xstudio_plugin_entry_v1(
    host: *const HostApiV1,
    output: *mut PluginDescriptorV1,
) -> PluginStatus {
    if host.is_null() || output.is_null() {
        return PluginStatus::InvalidArgument;
    }
    let host = unsafe { &*host };
    if host.abi_version != PLUGIN_ABI_VERSION {
        return PluginStatus::NotSupported;
    }

    let state = Box::new(PluginState {
        host_log: host.log,
        host_env_call: host.env_call,
        host_context: host.context,
    });
    log(&state, "info", "示例插件已初始化");

    unsafe {
        *output = PluginDescriptorV1 {
            abi_version: PLUGIN_ABI_VERSION,
            plugin_context: Box::into_raw(state).cast(),
            manifest: json_response(plugin_manifest()),
            call: xstudio_plugin_call_v1,
        };
    }
    PluginStatus::Ok
}

/// 处理宿主发送的 JSON 请求。
///
/// @param plugin_context 入口创建的 `PluginState` 指针。
/// @param request 宿主拥有的 JSON 字节。
/// @param output 由插件填写的响应缓冲区。
unsafe extern "C" fn xstudio_plugin_call_v1(
    plugin_context: *mut c_void,
    request: JsonBytes,
    output: *mut PluginJsonBytes,
) -> PluginStatus {
    if plugin_context.is_null() || output.is_null() || (request.len > 0 && request.data.is_null()) {
        return PluginStatus::InvalidArgument;
    }
    let state = unsafe { &*(plugin_context.cast::<PluginState>()) };
    let request_bytes = unsafe { slice::from_raw_parts(request.data, request.len) };
    let request: Value = match serde_json::from_slice(request_bytes) {
        Ok(request) => request,
        Err(error) => {
            log(state, "error", &format!("请求 JSON 无效: {error}"));
            return PluginStatus::InvalidArgument;
        }
    };

    let host_env = HostEnvApi::new(state.host_context, state.host_env_call);
    match dispatch(request, host_env) {
        Ok(response) => {
            unsafe { *output = json_response(response) };
            PluginStatus::Ok
        }
        Err((status, message)) => {
            log(state, "error", &message);
            status
        }
    }
}

/// 序列化并调用宿主日志回调。
///
/// @param state 插件进程级状态。
/// @param level 日志等级。
/// @param message 日志文本。
fn log(state: &PluginState, level: &str, message: &str) {
    let event = serde_json::to_vec(&json!({ "level": level, "message": message }))
        .expect("example log event must serialize");
    unsafe {
        (state.host_log)(
            state.host_context,
            JsonBytes {
                data: event.as_ptr(),
                len: event.len(),
            },
        );
    }
}
