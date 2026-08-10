# 快速开始

`plugin-api` 目录本身是可编译、可测试的最小原生插件案例。实际开发时直接复制整个目录，在副本中替换示例清单和业务分发即可。

## 复制案例

复制整个 `plugin-api` 目录并为副本改名，例如：

```text
cp -R plugin-api my-xstudio-plugin
```

案例的 `Cargo.toml` 已同时配置 `rlib` 和 `cdylib`：前者让宿主 workspace 可以链接 ABI 类型，后者生成可部署的动态库。`src/entry.rs` 已导出固定符号 `xstudio_plugin_entry_v1`，不要修改该名称。

在副本中主要修改：

- `src/manifest.rs`：仅保留实际实现的贡献；
- `src/dispatch.rs`：实现对应能力的请求处理；
- `src/entry.rs`：按需扩充由插件拥有的 `PluginState`。

## 编译和部署

编译 release 动态库：

```text
cargo build --release
```

先运行案例测试，确认 ABI 入口和统一调用回调正常：

```text
cargo test
```

将 `target/release` 内的库复制到 `<app_dir>/plugins`：

| 平台 | 典型文件名 |
| --- | --- |
| macOS | `libmy_xstudio_plugin.dylib` |
| Windows | `my_xstudio_plugin.dll` |
| Linux | `libmy_xstudio_plugin.so` |

重启 XStudio 后，宿主扫描该目录并加载插件。

## 从最小实现开始

初次开发建议只保留清单的 `id`，并使分发函数对任何调用返回 `NotSupported`。确认动态库可被加载后，再逐项添加能力：

1. 在清单声明贡献。
2. 在 `dispatch` 添加对应 `kind` / `operation` 分支。
3. 返回符合宿主 Rust 类型序列化格式的 JSON。
4. 构建、部署并重启应用验证。

详见 [清单与注册](./manifest.md) 和 [能力协议](./capabilities.md)。