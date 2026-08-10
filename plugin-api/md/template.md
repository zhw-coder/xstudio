# 模板代码说明

`plugin-api/src` 本身就是可编译、可测试的分模块插件案例。复制整个 `plugin-api` 目录后，可在副本中继续完成实际插件。案例故意采用空实现或占位响应，避免把示例误当成完整业务插件。

## 文件职责

| 文件 | 职责 |
| --- | --- |
| `manifest.rs` | 构建插件 JSON 清单；删除没有实现的能力。 |
| `response.rs` | 把 `serde_json::Value` 转换为插件拥有、可由宿主释放的 `PluginJsonBytes`。 |
| `dispatch.rs` | 解析 `kind` / `operation`，并分发 Env、Provider、Tool、Search、Harness 请求。 |
| `entry.rs` | 导出 `xstudio_plugin_entry_v1`，校验宿主 ABI，创建状态并调用 dispatcher。 |

## 复制注意事项

1. 复制整个 `plugin-api` 目录到插件项目位置。
2. 在 `manifest.rs` 删除未实现的贡献。
3. 在 `dispatch.rs` 为保留的贡献实现参数验证和实际业务逻辑。
4. 保留 `lib.rs` 的 ABI 类型与 `entry.rs` 的 `#[no_mangle] pub unsafe extern "C" fn xstudio_plugin_entry_v1`。
5. 执行 `cargo test`，再执行 `cargo build --release`。

## 响应内存所有权

模板的 `json_response` 使用 `ManuallyDrop<Vec<u8>>` 将 JSON 字节的所有权交给 `PluginJsonBytes`。当宿主完成读取，会调用模板提供的 `free_json_bytes`，通过 `Vec::from_raw_parts` 恢复并释放相同分配。

因此：

- 只能为由 Rust `Vec<u8>` 分配的响应使用此释放器。
- 不要对静态字符串、外部库内存或已释放内存设置该 `free` 函数。
- 失败状态不应生成需要宿主释放的半成品输出。

## 状态扩展

模板的 `PluginState` 只保存宿主日志回调。实际插件可以加入配置、连接池或客户端，但字段必须由插件自身拥有。不得保存请求 `JsonBytes` 的裸指针，也不得将宿主回调的临时字节切片长期保存。

v1 没有析构回调，长期状态会一直存活到进程退出；按此生命周期设计资源。