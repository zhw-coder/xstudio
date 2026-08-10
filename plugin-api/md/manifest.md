# 清单与注册

入口成功时，`PluginDescriptorV1.manifest` 必须返回 JSON 清单。宿主用它决定插件的能力与注册表贡献。

## 完整示例

```json
{
  "id": "com.example.sample",
  "capabilities": ["env", "harness"],
  "providers": [{ "name": "sample-api" }],
  "tools": [{
    "name": "sample-tool",
    "definition": {
      "name": "sample-tool",
      "description": "示例工具",
      "parameters": { "type": "object", "properties": {} }
    },
    "executionMode": "parallel"
  }],
  "searches": [{ "name": "sample-search", "domain": "general" }]
}
```

## 字段说明

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `id` | 是 | 插件稳定标识，建议使用反向域名。 |
| `capabilities` | 否 | 目前仅使用 `env` 与 `harness`。 |
| `providers` | 否 | Provider 名称列表，注册至 `ApiRegistry`。 |
| `tools` | 否 | 工具名称、`definition` 和可选 `executionMode`，注册至 `ToolRegistry`。 |
| `searches` | 否 | 搜索名称和非空 `domain`，注册至 `SearchRegistry`。 |

`providers`、`tools`、`searches` 缺省时按空数组处理。仅清单中列出的贡献才会被宿主注册；仅实现分发分支不会自动暴露能力。

## 覆盖规则

同名 Provider、Tool 或 Search 贡献采用后写入覆盖先写入：后加载的插件会替代前加载插件或内置实现。插件运行时不进行冲突诊断或排序；部署者应控制插件目录内容与加载顺序。

## 当前限制

- `env`：如果多个插件声明，宿主仅选择第一个声明者；未声明时回退本地执行环境。
- `provider`：v1 不支持 token 级流式响应。
- `tool`：v1 不支持执行过程中的更新回调。
- `search`：`domain` 必须为非空字符串。
- `session.repo`：当前没有动态插件适配，不能在清单声明。

配置字段使用 camelCase，例如 `executionMode`。模板 `manifest.rs` 已展示各字段的序列化形式。