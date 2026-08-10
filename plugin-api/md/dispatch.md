# 调用分发

宿主通过同一个 `PluginCallV1` 函数调用插件。每个请求都是 JSON 对象，顶层使用 `kind`、`operation`，Provider、Tool 和 Search 还使用 `name`：

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

模板的 `dispatch(request)` 先校验 `kind` 和 `operation`，然后转发给 `handle_env`、`handle_provider`、`handle_tool`、`handle_search` 或 `handle_harness`。

## 实现原则

- 先验证顶层字段与 `arguments` 的 JSON 类型，再读取业务参数。
- 未识别的能力或操作返回 `PluginStatus::NotSupported`。
- 参数缺失、格式错误或无效 JSON 返回 `PluginStatus::InvalidArgument`。
- 已识别但执行失败时记录错误堆栈/诊断，并返回 `PluginStatus::Failed`。
- 成功时必须填充 `PluginJsonBytes`；即使返回值是空，也返回 JSON `null`。
- 返回值必须匹配宿主目标 Rust 类型的 serde JSON 表示，不能自定义未经约定的包装层。

## 按名称分发

一个插件可以声明多个 Provider、Tool 或 Search。应先读取 `name`，再由名称与操作共同分发：

```rust
match (name, operation) {
    ("example-tool", "execute") => execute_example_tool(arguments),
    _ => Err((PluginStatus::NotSupported, "未知插件贡献".to_string())),
}
```

Env 和 Harness 不使用贡献名。Env 的 `cwd` 位于请求顶层；Harness hook 名位于顶层 `name`。

## 线程与阻塞

宿主会将大部分插件调用安排到阻塞任务中，但 ABI 本身仍是同步函数。插件不应把宿主传入的裸指针保存到异步任务；如需异步工作，应先复制并拥有需要的 JSON 数据，再在返回响应前完成任务。