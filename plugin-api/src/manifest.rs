//! 示例插件清单。
//!
//! 复制本目录作为新插件项目后，在此处保留实际实现的能力并删除其余贡献。

use serde_json::{json, Value};

/// 创建示例插件清单。
pub fn plugin_manifest() -> Value {
    json!({
        "id": "com.example.xstudio-plugin",
        "capabilities": [
            // "env",
            "harness"
        ],
        "providers": [{ "name": "example-provider" }],
        "tools": [{
            "name": "echo",
            "definition": {
                "name": "echo",
                "description": "返回传入的消息。",
                "parameters": {
                    "type": "object",
                    "properties": { "message": { "type": "string" } }
                }
            },
            "executionMode": "parallel"
        }],
        "searches": [{ "name": "example-search", "domain": "general" }]
    })
}
