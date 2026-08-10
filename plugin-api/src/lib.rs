//! XStudio 原生插件 ABI 与可运行示例。
//!
//! 复制整个 `plugin-api` 目录后，即可在独立项目中继续开发原生 `cdylib` 插件。

use std::ffi::c_void;

mod dispatch;
pub mod entry;
mod manifest;
mod response;

/// 当前插件 ABI 主版本。
pub const PLUGIN_ABI_VERSION: u32 = 1;
/// 插件导出的固定入口符号。
pub const PLUGIN_ENTRY_SYMBOL: &[u8] = b"xstudio_plugin_entry_v1\0";

/// C ABI 调用的结果状态码。
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginStatus {
    /// 调用成功。
    Ok = 0,
    /// 请求参数不合法。
    InvalidArgument = 1,
    /// 插件不支持该操作。
    NotSupported = 2,
    /// 插件执行失败。
    Failed = 3,
}

/// 由调用方拥有的 UTF-8 JSON 字节片段。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JsonBytes {
    /// UTF-8 JSON 的起始地址。
    pub data: *const u8,
    /// UTF-8 JSON 字节长度。
    pub len: usize,
}

/// 由插件分配并由其释放的 UTF-8 JSON 字节片段。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PluginJsonBytes {
    /// UTF-8 JSON 的起始地址。
    pub data: *const u8,
    /// UTF-8 JSON 字节长度。
    pub len: usize,
    /// 释放本片段的插件函数；可为空表示空结果。
    pub free: Option<unsafe extern "C" fn(data: *const u8, len: usize)>,
}

/// 宿主向插件提供的版本化函数表。
#[repr(C)]
pub struct HostApiV1 {
    /// 宿主 ABI 版本。
    pub abi_version: u32,
    /// 宿主上下文，由所有回调原样传回。
    pub context: *mut c_void,
    /// 记录插件诊断 JSON：{"level":"error|warn|info","message":"..."}。
    pub log: unsafe extern "C" fn(context: *mut c_void, event: JsonBytes),
    /// 使用宿主默认 ExecutionEnv 处理 JSON 请求并返回 JSON 响应。
    ///
    /// 请求需含 `cwd`、`operation`、`arguments`。返回缓冲区由宿主分配，插件必须调用
    /// `free` 回调释放。该回调可在 Provider、Tool、Search 与 Harness hook 内调用。
    pub env_call: unsafe extern "C" fn(
        context: *mut c_void,
        request: JsonBytes,
        output: *mut PluginJsonBytes,
    ) -> PluginStatus,
}

/// 插件的同步 JSON 调用函数。
///
/// 输入与输出必须是约定版本的 UTF-8 JSON。插件必须在返回前填充 `output`。
pub type PluginCallV1 = unsafe extern "C" fn(
    plugin_context: *mut c_void,
    request: JsonBytes,
    output: *mut PluginJsonBytes,
) -> PluginStatus;

/// 插件的版本化描述符。
#[repr(C)]
pub struct PluginDescriptorV1 {
    /// 插件 ABI 版本，必须等于 `PLUGIN_ABI_VERSION`。
    pub abi_version: u32,
    /// 插件实例上下文，由宿主回传到 callback。
    pub plugin_context: *mut c_void,
    /// 描述插件和声明能力的 UTF-8 JSON。
    pub manifest: PluginJsonBytes,
    /// 调用插件能力的统一入口。
    pub call: PluginCallV1,
}

/// 插件必须导出的入口函数类型。
pub type PluginEntryV1 =
    unsafe extern "C" fn(host: *const HostApiV1, output: *mut PluginDescriptorV1) -> PluginStatus;
