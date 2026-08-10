//! xstudio 内置 Agent 工具。

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use ai::agent::{AgentTool, AgentToolError};
use serde_json::Value;

pub mod bash;
pub mod edit;
pub mod fetch;
pub mod find;
pub mod grep;
pub mod ls;
pub mod read;
pub mod search;
pub mod write;

pub mod lib {
    pub mod edit_diff;
    pub mod file_mutation_queue;
    pub mod truncate;
}

pub use bash::BashTool;
pub use edit::EditTool;
pub use fetch::FetchTool;
pub use find::FindTool;
pub use grep::GrepTool;
pub use ls::LsTool;
pub use read::ReadTool;
pub use search::{SearchRegistry, SearchTool};
pub use write::WriteTool;

/// Agent 工具注册表。
#[derive(Clone)]
pub struct ToolRegistry {
    /// 按工具名称保存全局共享的工具实例。
    tools: HashMap<String, Arc<dyn AgentTool>>,
}

/// 全局静态工具注册表。
static GLOBAL_TOOL_REGISTRY: OnceLock<Arc<ToolRegistry>> = OnceLock::new();

impl ToolRegistry {
    /// 创建并注册全部内置工具。
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
        };
        registry.register(BashTool::name(), Arc::new(BashTool::new()));
        registry.register(EditTool::name(), Arc::new(EditTool::new()));
        registry.register(FetchTool::name(), Arc::new(FetchTool::new()));
        registry.register(FindTool::name(), Arc::new(FindTool::new()));
        registry.register(GrepTool::name(), Arc::new(GrepTool::new()));
        registry.register(LsTool::name(), Arc::new(LsTool::new()));
        registry.register(ReadTool::name(), Arc::new(ReadTool::new()));
        registry.register(SearchTool::name(), Arc::new(SearchTool::new()));
        registry.register(WriteTool::name(), Arc::new(WriteTool::new()));
        registry
    }

    /// 返回全局静态单例注册表。
    pub fn global() -> &'static Arc<Self> {
        GLOBAL_TOOL_REGISTRY.get_or_init(|| Arc::new(Self::new()))
    }

    /// 使用内置工具和启动期扩展初始化全局注册表。
    ///
    /// 必须在首次调用 `global` 前执行；同名扩展会覆盖内置工具。
    /// @param extensions 工具名称到工具实例的映射。
    pub fn global_with_extensions(
        extensions: HashMap<String, Arc<dyn AgentTool>>,
    ) -> Result<&'static Arc<Self>, String> {
        let mut registry = Self::new();
        for (name, tool) in extensions {
            registry.register(name, tool);
        }
        GLOBAL_TOOL_REGISTRY
            .set(Arc::new(registry))
            .map_err(|_| "ToolRegistry has already been initialized".to_string())?;
        Ok(GLOBAL_TOOL_REGISTRY
            .get()
            .expect("ToolRegistry must be available after initialization"))
    }

    /// 注册工具。
    /// @param name 工具名称。
    /// @param tool 工具实例。
    pub fn register(&mut self, name: impl Into<String>, tool: Arc<dyn AgentTool>) {
        self.tools.insert(name.into(), tool);
    }

    /// 按名称获取已注册工具。
    pub fn get(&self, name: &str) -> Option<Arc<dyn AgentTool>> {
        self.tools.get(name).map(Arc::clone)
    }

    /// 返回全部已注册工具名称。
    pub fn names(&self) -> Vec<String> {
        let mut names = self.tools.keys().cloned().collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    /// 使用配置初始化对应的已注册工具。
    /// @param configs 工具名称到工具专属 JSON 配置的映射。
    pub fn init(&self, configs: HashMap<String, Value>) -> Result<(), AgentToolError> {
        for (name, configs) in configs {
            let tool = self
                .get(&name)
                .ok_or_else(|| AgentToolError::Message(format!("Unknown tool: {name}")))?;
            tool.init(configs)?;
        }
        Ok(())
    }

    /// 返回全部已注册工具。
    pub fn tools(&self) -> Vec<Arc<dyn AgentTool>> {
        let mut tools = Vec::new();
        for name in self.names() {
            let tool = self
                .get(&name)
                .expect("ToolRegistry names are sourced from registered tools");
            tools.push(tool);
        }
        tools
    }
}
