# ABI 与入口

## 固定入口

插件必须导出以下 C ABI 符号：

```rust
#[no_mangle]
pub unsafe extern "C" fn xstudio_plugin_entry_v1(
    host: *const HostApiV1,
    output: *mut PluginDescriptorV1,
) -> PluginStatus
```

入口必须：

1. 检查 `host` 与 `output` 非空。
2. 检查 `host.abi_version == PLUGIN_ABI_VERSION`。
3. 创建并保留插件状态，必要时通过 `Box::into_raw` 写入 `plugin_context`。
4. 填写 ABI 版本、JSON 清单和统一 `call` 函数。
5. 返回 `PluginStatus::Ok`。

不要改动 v1 结构体字段顺序、类型或入口符号。需要不兼容变更时，应设计新 ABI 版本和新入口符号。

## JSON 输入和输出

宿主调用：

```rust
unsafe extern "C" fn call(
    plugin_context: *mut c_void,
    request: JsonBytes,
    output: *mut PluginJsonBytes,
) -> PluginStatus
```

`request` 是宿主拥有的 UTF-8 JSON 字节，仅在本次调用期间有效。插件应在调用内反序列化，不能保存该指针。

`output` 是插件填写的响应。推荐使用模板的 `json_response`：它将 JSON 写入 `Vec<u8>`，把 `Vec` 通过 `ManuallyDrop` 转换为裸指针，并提供对称的 `free` 回调。宿主调用回调释放内存，因此 `free` 必须使用插件相同的 Rust 分配器。

成功响应示例：

```rust
*output = json_response(serde_json::json!({"ok": true}));
PluginStatus::Ok
```

失败时记录诊断并返回相应 `PluginStatus`；不要填入部分初始化的输出。

## 状态码

| 状态 | 使用场景 |
| --- | --- |
| `Ok` | 请求成功，且已填写有效 JSON 输出。 |
| `InvalidArgument` | 空指针、无效 JSON、必填字段缺失或类型错误。 |
| `NotSupported` | 未声明或尚未实现的能力、操作或 ABI 版本。 |
| `Failed` | 已识别请求，但执行过程中发生内部失败。 |

## 日志

`HostApiV1.log` 可接收 JSON 事件：

```json
{"level":"info","message":"插件已初始化"}
```

该回调与入口提供的 `host.context` 配对使用。日志字节只需在回调返回前有效，临时 `Vec<u8>` 即可。

## 宿主默认 Env

`HostApiV1.env_call` 允许插件反向调用宿主默认 `LocalExecutionEnv`，不会替换宿主执行环境。请求必须含 `cwd`、`operation` 与 `arguments`：

```json
{
    "cwd": "/project",
    "operation": "read_text_file",
    "arguments": { "path": "README.md" }
}
```

回调是同步的，成功时返回宿主分配的 JSON 字节。插件读取后必须调用 `PluginJsonBytes.free` 释放缓冲区。案例将该回调封装为 `HostEnvApi` 并传给 `handle_harness`；在 hook 实现中调用 `host_env.call(...)` 即可使用宿主默认 Env。

## 生命周期

宿主会一直持有动态库，不会回收 `plugin_context`。因此 v1 插件状态应当进程级复用，避免把短期借用或单次请求指针存入状态。