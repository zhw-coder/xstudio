# 能力协议

本页列出宿主当前调用的能力。所有成功响应都是 JSON；错误使用 ABI 状态码返回。

## Env

请求顶层含 `cwd`：

```json
{
  "kind": "env",
  "operation": "read_text_file",
  "cwd": "/project",
  "arguments": { "path": "README.md" }
}
```

操作对应 `ExecutionEnv` 异步方法，包括 `exec`、`read_text_file`、`read_text_range`、`read_binary_file`、`read_binary_range`、`write_file`、`file_info`、`list_dir`、`find_files`、`real_path`、`exists`、`create_dir`、`remove`、`create_temp_dir`、`create_temp_file`、`create_point`、`get_point`、`reset_point` 与 `cleanup`。

响应必须是相应 Rust 返回值的 serde JSON 形式。例如 `exists` 返回布尔值，`read_text_file` 返回字符串，`write_file` 与 `cleanup` 返回 `null`。路径的 `resolve_path`、`join_path`、`dirname_path`、`basename_path`、`relative_path` 在宿主侧执行，不会发给插件。

案例插件不声明 `env` capability，因此不会替换宿主执行环境。Harness hook 如需访问项目文件，可通过入口传入的 `HostApiV1.env_call` 调用宿主默认 `LocalExecutionEnv`；案例在 `dispatch.rs` 中将其封装为 `HostEnvApi` 并传给 `handle_harness`。

## Provider

请求：

```json
{
  "kind": "provider",
  "name": "example-provider",
  "operation": "models",
  "arguments": {}
}
```

支持 `models`、`stream` 与 `streamSimple`。`models` 返回 `Vec<Model>` 的 JSON；两个 stream 操作当前必须返回完整 `AssistantMessage` JSON。v1 不支持逐 token 或增量事件。

案例 `example-provider` 已实现：`models` 返回 `example-chat` 的模型元数据，`stream` 和 `streamSimple` 返回最后一条用户文本的完整 `AssistantMessage`。复制后应在 `dispatch.rs` 中替换为实际服务调用，并保留 `AssistantMessage` 的完整 JSON 字段。

## Tool

支持 `init` 与 `execute`：

```json
{
  "kind": "tool",
  "name": "example-tool",
  "operation": "execute",
  "arguments": {
    "cwd": "/project",
    "toolCallId": "call-1",
    "params": { "message": "hello" }
  }
}
```

`init` 接收工具配置并返回 `null`。`execute` 必须返回 `AgentToolResult` 的 JSON 表示。v1 不支持 `UpdateToolCallHook`，因此插件无法推送工具运行中的增量更新。

案例清单声明 `echo`，其实现不依赖固定工具名：读取 `arguments.cwd`、`arguments.toolCallId` 与 `arguments.params.message`，并返回一个文本 `AgentToolResult`。复制后若要提供多个工具，可在 `dispatch.rs` 中根据顶层 `name` 再分发；不要沿用案例名称。

## Search

支持 `parameters`、`init` 与 `search`：

```json
{
  "kind": "search",
  "name": "example-search",
  "operation": "search",
  "arguments": { "query": "Rust ownership" }
}
```

`parameters` 返回当前配置 JSON，`init` 返回 `null`，`search` 返回结果文本字符串。HTTP 客户端由宿主内置搜索实现使用，插件 Search 不会收到 Rust `reqwest::Client`。

## Harness

仅声明 `"harness"` capability 后可用。

### 事件通知

```json
{
  "kind": "harness",
  "operation": "event",
  "arguments": { "type": "..." }
}
```

事件响应不会影响宿主，但仍必须为合法 JSON，通常返回 `null`。

### 控制型 Hook

```json
{
  "kind": "harness",
  "operation": "hook",
  "name": "context",
  "arguments": { "event": {} }
}
```

支持：`beforeAgentStart`、`context`、`beforeProviderRequest`、`beforeProviderPayload`、`afterProviderResponse`、`toolCall`、`toolResult`。

返回 `null` 表示不修改。非 `null` 返回必须符合该 hook 原生返回类型的 JSON；多个插件按加载顺序执行，最后一个非 `null` 返回值生效。工具调用场景中，用户审批 hook 在插件 hook 后执行，因此审批拒绝优先。

Harness 事件中存在宿主借用字段，宿主会先生成拥有所有权的 JSON 快照。插件只能基于该快照工作，不能假设可访问 Rust 原始事件对象。