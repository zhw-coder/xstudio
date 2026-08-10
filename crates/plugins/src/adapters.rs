use std::{fmt, sync::Arc};

use ai::{
    agent::{
        env::ExecutionEnv, AgentTool, AgentToolError, AgentToolResult, ToolExecutionMode,
        UpdateToolCallHook,
    },
    model::{
        ApiProvider, AssistantMessage, AssistantMessageEvent, AssistantMessageEventSink, Auth,
        Context, Model, StreamError, StreamOptions, Tool,
    },
};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tool::search::SearchEngine;

use crate::runtime::PluginHandle;

/// 插件清单中的可注册能力描述。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginContribution {
    /// 宿主注册表中的名称。
    pub name: String,
    /// 搜索能力所属领域。
    #[serde(default)]
    pub domain: String,
    /// Tool 定义。
    #[serde(default)]
    pub definition: Option<Tool>,
    /// Tool 执行模式。
    #[serde(default)]
    pub execution_mode: Option<ToolExecutionMode>,
}

/// 插件 Provider 适配器。
pub struct PluginApiProvider {
    plugin: Arc<PluginHandle>,
    name: String,
}

impl PluginApiProvider {
    /// 创建指定插件 Provider 代理。
    /// @param plugin 提供能力的插件。
    /// @param name Provider 注册名称。
    pub fn new(plugin: Arc<PluginHandle>, name: String) -> Self {
        Self { plugin, name }
    }

    /// 调用插件并转换为流错误。
    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        operation: &str,
        request: Value,
    ) -> Result<T, StreamError> {
        let plugin = Arc::clone(&self.plugin);
        let name = self.name.clone();
        let operation = operation.to_string();
        tokio::task::spawn_blocking(move || {
            plugin.call(
                json!({"kind":"provider","name":name,"operation":operation,"arguments":request}),
            )
        })
        .await
        .map_err(|error| StreamError::Stream(error.to_string()))?
        .map_err(|error| StreamError::Stream(error.to_string()))
        .and_then(|value| {
            serde_json::from_value(value).map_err(|error| StreamError::Stream(error.to_string()))
        })
    }

    /// 将非流式插件结果作为完成事件提交给宿主 sink。
    async fn stream_message(
        &self,
        operation: &str,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        let message: AssistantMessage = self
            .call(
                operation,
                json!({"model":model,"context":context,"options":options,"auth":auth}),
            )
            .await?;
        sink.emit(AssistantMessageEvent::Done {
            reason: message.stop_reason.clone(),
            message: message.clone(),
        })
        .await
    }
}

#[async_trait]
impl ApiProvider for PluginApiProvider {
    async fn models(
        &self,
        provider: &str,
        base_url: &str,
        options: &StreamOptions,
        auth: &Auth,
    ) -> Result<Vec<Model>, StreamError> {
        self.call(
            "models",
            json!({"provider":provider,"baseUrl":base_url,"options":options,"auth":auth}),
        )
        .await
    }

    async fn stream(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        self.stream_message("stream", model, context, options, auth, sink)
            .await
    }

    async fn stream_simple(
        &self,
        model: &Model,
        context: Context,
        options: &StreamOptions,
        auth: &Auth,
        sink: &mut dyn AssistantMessageEventSink,
    ) -> Result<AssistantMessage, StreamError> {
        self.stream_message("streamSimple", model, context, options, auth, sink)
            .await
    }
}

/// 插件 AgentTool 适配器。
pub struct PluginAgentTool {
    plugin: Arc<PluginHandle>,
    name: String,
    definition: Tool,
    execution_mode: Option<ToolExecutionMode>,
}

impl fmt::Debug for PluginAgentTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginAgentTool")
            .field("name", &self.name)
            .finish()
    }
}

impl PluginAgentTool {
    /// 创建指定插件工具代理。
    /// @param plugin 提供能力的插件。
    /// @param contribution 工具清单描述。
    pub fn new(
        plugin: Arc<PluginHandle>,
        contribution: PluginContribution,
    ) -> Result<Self, String> {
        let definition = contribution
            .definition
            .ok_or_else(|| format!("插件工具 {} 缺少 definition", contribution.name))?;
        Ok(Self {
            plugin,
            name: contribution.name,
            definition,
            execution_mode: contribution.execution_mode,
        })
    }
}

#[async_trait]
impl AgentTool for PluginAgentTool {
    fn new() -> Self
    where
        Self: Sized,
    {
        panic!("PluginAgentTool must be created from a plugin manifest")
    }
    fn name() -> &'static str
    where
        Self: Sized,
    {
        "plugin"
    }
    fn definition(&self) -> Tool {
        self.definition.clone()
    }
    fn init(&self, configs: Value) -> Result<(), AgentToolError> {
        self.plugin
            .call(json!({"kind":"tool","name":self.name,"operation":"init","arguments":configs}))
            .map(|_| ())
            .map_err(|error| AgentToolError::Message(error.to_string()))
    }
    async fn execute(
        &self,
        env: &dyn ExecutionEnv,
        tool_call_id: &String,
        params: &Value,
        _on_update: Option<&UpdateToolCallHook>,
    ) -> Result<AgentToolResult, AgentToolError> {
        let plugin = Arc::clone(&self.plugin);
        let name = self.name.clone();
        let request = json!({"kind":"tool","name":name,"operation":"execute","arguments":{"cwd":env.cwd(),"toolCallId":tool_call_id,"params":params}});
        tokio::task::spawn_blocking(move || plugin.call(request))
            .await
            .map_err(|error| AgentToolError::Message(error.to_string()))?
            .map_err(|error| AgentToolError::Message(error.to_string()))
            .and_then(|value| {
                serde_json::from_value(value)
                    .map_err(|error| AgentToolError::Message(error.to_string()))
            })
    }
    fn execution_mode(&self) -> Option<ToolExecutionMode> {
        self.execution_mode.clone()
    }
}

/// 插件搜索引擎适配器。
pub struct PluginSearchEngine {
    plugin: Arc<PluginHandle>,
    name: String,
    domain: String,
}

impl fmt::Debug for PluginSearchEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PluginSearchEngine")
            .field("name", &self.name)
            .field("domain", &self.domain)
            .finish()
    }
}

impl PluginSearchEngine {
    /// 创建指定插件搜索引擎代理。
    /// @param plugin 提供能力的插件。
    /// @param contribution 搜索清单描述。
    pub fn new(
        plugin: Arc<PluginHandle>,
        contribution: PluginContribution,
    ) -> Result<Self, String> {
        if contribution.domain.is_empty() {
            return Err(format!("插件搜索引擎 {} 缺少 domain", contribution.name));
        }
        Ok(Self {
            plugin,
            name: contribution.name,
            domain: contribution.domain,
        })
    }

    /// 调用搜索插件并转换错误。
    fn call(&self, operation: &str, arguments: Value) -> Result<Value, AgentToolError> {
        self.plugin.call(json!({"kind":"search","name":self.name,"operation":operation,"arguments":arguments}))
            .map_err(|error| AgentToolError::Message(error.to_string()))
    }
}

#[async_trait]
impl SearchEngine for PluginSearchEngine {
    fn new() -> Self
    where
        Self: Sized,
    {
        panic!("PluginSearchEngine must be created from a plugin manifest")
    }
    fn name() -> &'static str
    where
        Self: Sized,
    {
        "plugin"
    }
    fn domain(&self) -> &str {
        &self.domain
    }
    fn parameters(&self) -> Result<Value, AgentToolError> {
        self.call("parameters", Value::Null)
    }
    fn init(&self, parameters: Value) -> Result<(), AgentToolError> {
        self.call("init", parameters).map(|_| ())
    }
    async fn search(&self, _client: &Client, query: &str) -> Result<String, AgentToolError> {
        let plugin = Arc::clone(&self.plugin);
        let name = self.name.clone();
        let query = query.to_string();
        tokio::task::spawn_blocking(move || plugin.call(json!({"kind":"search","name":name,"operation":"search","arguments":{"query":query}})))
            .await
            .map_err(|error| AgentToolError::Message(error.to_string()))?
            .map_err(|error| AgentToolError::Message(error.to_string()))
            .and_then(|value| serde_json::from_value(value).map_err(|error| AgentToolError::Message(error.to_string())))
    }
}
