# XStudio 原生插件开发手册

本文档集面向编写 XStudio 原生动态库插件的开发者。`plugin-api` 本身是一个可编译、可测试的 `cdylib` 示例；复制整个目录后即可在新项目中继续开发。宿主运行时实现在 `crates/plugins`。

## 文档导航

- [快速开始](./quick-start.md)：创建可编译、可加载的最小插件。
- [ABI 与入口](./abi.md)：C ABI 规则、入口函数、JSON 字节缓冲区与内存释放。
- [清单与注册](./manifest.md)：声明插件 ID、Env、Provider、Tool 与 Search 贡献。
- [调用分发](./dispatch.md)：按 `kind`、`operation` 和 `name` 实现 JSON 请求处理。
- [能力协议](./capabilities.md)：Env、Provider、Tool、Search、Harness 的请求与响应要求。
- [模板代码](./template.md)：`plugin-api/src/template` 中各模块的职责和复制方式。

## 运行模型

1. 宿主启动时扫描 `<app_dir>/plugins`。
2. 宿主加载当前平台动态库：macOS `.dylib`、Windows `.dll`、Linux/其他 Unix `.so`。
3. 宿主查找导出符号 `xstudio_plugin_entry_v1`，并校验 ABI v1。
4. 插件返回 JSON 清单和统一 `PluginCallV1` 回调。
5. 宿主在首次读取全局注册表前装配插件贡献，并一直持有动态库句柄到进程退出。

插件没有热更新或卸载机制。用户替换插件动态库后需要重启应用。

## 关键限制

- 只跨 ABI 传递 `#[repr(C)]` 结构体、标量、裸指针和 UTF-8 JSON 字节。
- 不得跨 ABI 传递 Rust trait object、`String`、`Vec`、Future、HashMap 或借用引用。
- 插件返回的 `PluginJsonBytes` 必须设置释放函数；宿主无论是否解析成功都会调用它。
- `len > 0` 时 `data` 必须非空。
- 插件调用是同步的。耗时逻辑应由插件自行控制，避免无界阻塞宿主请求。
- 重名 Provider、Tool、Search 贡献由后加载插件覆盖；部署方负责插件文件顺序。

## 最小目录结构

```text
my-xstudio-plugin/
  Cargo.toml
  src/
    lib.rs
    manifest.rs
    dispatch.rs
    response.rs
```

直接复制整个 `plugin-api` 目录作为新插件项目，再删除未实现的能力声明与分发分支。`src/lib.rs` 保留稳定 ABI 类型，`src/entry.rs` 导出固定入口，其他 `src/*.rs` 文件是可替换的业务实现。

## 验证

## 测试约束

所有测试代码必须放在 `plugin-api/tests/`，使用集成测试方式通过公开 ABI 入口验证插件行为；禁止在 `plugin-api/src/` 内编写 `#[cfg(test)]` 测试模块。复制本目录开发新插件时也必须遵守此目录约定。

插件 crate 至少应完成：

- `cargo fmt`
- `cargo check`
- `cargo test`
- `cargo build --release`

将产物复制到应用数据目录的 `plugins` 子目录后，重启桌面应用验证加载结果。