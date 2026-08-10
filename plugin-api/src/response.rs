//! 插件 JSON 响应缓冲区。
//!
//! 宿主通过 `free` 回调归还插件分配的 JSON 字节；不要将 Rust `Vec` 直接跨 ABI 暴露。

use std::mem::ManuallyDrop;

use serde_json::Value;

use crate::PluginJsonBytes;

/// 将 JSON 值转为宿主可读取、插件负责释放的字节缓冲区。
///
/// @param value 要返回给宿主的 JSON 值。
pub fn json_response(value: Value) -> PluginJsonBytes {
    let bytes = ManuallyDrop::new(serde_json::to_vec(&value).expect("example JSON must serialize"));
    PluginJsonBytes {
        data: bytes.as_ptr(),
        len: bytes.len(),
        free: Some(free_json_bytes),
    }
}

/// 释放 `json_response` 分配的字节缓冲区。
///
/// @param data 原 JSON 字节起始地址。
/// @param len 原 JSON 字节长度。
unsafe extern "C" fn free_json_bytes(data: *const u8, len: usize) {
    if !data.is_null() {
        drop(unsafe { Vec::from_raw_parts(data.cast_mut(), len, len) });
    }
}
