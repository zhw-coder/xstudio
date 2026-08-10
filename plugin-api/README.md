# XStudio Plugin API v1

原生插件必须链接 `xstudio-plugin-api`，并导出 C 符号 `xstudio_plugin_entry_v1`。宿主从 `<app_dir>/plugins` 加载当前平台的动态库；插件只在应用启动时加载，运行中不支持卸载或热更新。

## 开发文档

- [原生插件开发手册](./md/core.md)
- [快速开始](./md/quick-start.md)
- [ABI 与入口](./md/abi.md)
- [清单与注册](./md/manifest.md)
- [调用分发](./md/dispatch.md)
- [能力协议](./md/capabilities.md)
- [模板代码说明](./md/template.md)

## ABI 约束

- ABI 版本必须为 `PLUGIN_ABI_VERSION = 1`。
- 仅可跨边界传递 `#[repr(C)]` 结构体、标量和 UTF-8 JSON 字节；不得传递 Rust trait object、`String`、`Vec`、Future 或 Rust 容器。
- 插件返回的 `PluginJsonBytes` 必须提供对应的 `free` 函数。即使宿主无法解析 JSON，也会调用该函数。
- `len > 0` 时 `data` 不能为 null。

## 清单

入口成功时通过 `PluginDescriptorV1.manifest` 返回：

```json
{
  "id": "com.example.sample",
  "capabilities": ["env", "harness"],
  "providers": [{ "name": "sample-api" }],
  "tools": [{
    "name": "sample-tool",
    "definition": { "name": "sample-tool", "description": "...", "parameters": {} },
    "executionMode": "parallel"
  }],
  "searches": [{ "name": "sample-search", "domain": "general" }]
}
```

后加载的同名贡献会覆盖内置实现或先加载插件的贡献。

## 调用协议

所有请求都经 `PluginCallV1` 发送，并以 JSON 响应。请求包含 `kind`、`operation`、可选 `name` 与 `arguments`：

- `env`：宿主在每个请求中传入 `cwd`，并在 `arguments` 中传递执行环境方法参数。
- `provider`：`models`、`stream`、`streamSimple`。当前 v1 为单次响应；`stream` 必须返回完整 `AssistantMessage`，宿主会发出最终 `done` 事件。
- `tool`：`init`、`execute`。执行请求含 `cwd`、`toolCallId`、`params`；返回 `AgentToolResult`。
- `search`：`parameters`、`init`、`search`；搜索返回文本字符串。
- `harness`：`event`，参数为可序列化的 Harness 事件快照。
- `harness`：`hook`，请求包含 hook `name` 及 `arguments.event` 快照。支持 `beforeAgentStart`、`context`、`beforeProviderRequest`、`beforeProviderPayload`、`afterProviderResponse`、`toolCall`、`toolResult`。返回 `null` 表示不修改；其他返回值必须分别符合对应 Rust hook 的 JSON 返回类型。插件按加载顺序调用，最后一个非空结果生效。

插件执行环境、Provider、工具或 Harness 事件回调应返回有效 JSON，即使返回值不被宿主使用。