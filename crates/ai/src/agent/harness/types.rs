//! Harness 框架使用的核心类型定义集合：Skill / 提示词模板等资源、执行环境、流式请求选项、
//! 会话条目协议、Harness 自身事件协议与各事件回调的返回值约定。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    ops::ControlFlow,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLockReadGuard;

use crate::{
    agent::{
        agent::PendingMessageQueue,
        types::{AfterToolCallResult, AgentError, AgentEvent, AgentMessage, BeforeToolCallResult, MHook},
    },
    model::{
        types::{ContentBlock, ImageContent, Model, StreamError, StreamOptions, ThinkingLevel},
        CacheRetention, Transport,
    },
};

/// `AgentHarness` 的主要错误。
#[derive(Debug, thiserror::Error)]
pub enum AgentHarnessError {
    /// Harness 通用错误。
    #[error(transparent)]
    Harness(#[from] crate::agent::harness::HarnessError),
    /// 底层 Agent 错误。
    #[error(transparent)]
    Agent(#[from] AgentError),
    /// 直接调用模型 stream 时的错误。
    #[error(transparent)]
    Stream(#[from] StreamError),
    /// 通用错误消息。
    #[error("{0}")]
    Message(String),
}

/// AgentHarness 结果类型。
pub type AgentHarnessRuntimeResult<T> = Result<T, AgentHarnessError>;

/// 从 `SKILL.md` 文件加载或由应用方直接提供的 Skill。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// 稳定 Skill 名称。
    pub name: String,
    /// 面向模型的简短描述。
    pub description: String,
    /// 完整 Skill 说明文本。
    pub content: String,
    /// Skill 文件的绝对路径。
    pub file_path: String,
    /// 为 true 时从模型可见列表排除。
    #[serde(default, skip_serializing_if = "is_false")]
    pub disable_model_invocation: bool,
}

/// 可被显式调用并格式化为 prompt 的提示词模板。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplate {
    /// 稳定模板名。
    pub name: String,
    /// 可选描述。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 模板正文。
    pub content: String,
}

/// 提供给显式调用方法与系统提示词回调使用的资源集合。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentHarnessResources {
    /// 提示词模板列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_templates: Vec<PromptTemplate>,
    /// Skill 列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<Skill>,
}

/// 由 Harness 持有并按轮快照的精选 Provider 请求选项集合。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentHarnessStreamOptions {
    /// 优先 Transport。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    /// 超时时间毫秒。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// 最大重试次数。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    /// 最大重试延迟。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retry_delay_ms: Option<u64>,
    /// 额外 headers。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Provider metadata。
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
    /// 缓存保留策略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_retention: Option<CacheRetention>,
}

/// 会话树条目的公共基础字段。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeEntryBase {
    /// 条目 id。
    pub id: String,
    /// 父条目 id。
    pub parent_id: Option<String>,
    /// ISO 8601 时间字符串。
    pub timestamp: String,
}

/// 会话树中允许的全部条目变体。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionTreeEntry {
    /// AgentMessage 追加条目。
    #[serde(rename = "message")]
    Message {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        message: AgentMessage,
    },
    /// thinking level 切换。
    #[serde(rename = "thinking_level_change")]
    ThinkingLevelChange {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        thinking_level: String,
    },
    /// model 切换。
    #[serde(rename = "model_change")]
    ModelChange {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        provider: String,
        model_id: String,
    },
    /// 上下文压缩条目。
    #[serde(rename = "compaction")]
    Compaction {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        summary: String,
        first_kept_entry_id: String,
        tokens_before: u64,
        details: Option<Value>,
        from_hook: Option<bool>,
    },
    /// 分支总结条目。
    #[serde(rename = "branch_summary")]
    BranchSummary {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        from_id: String,
        summary: String,
        details: Option<Value>,
        from_hook: Option<bool>,
    },
    /// 自定义条目。
    #[serde(rename = "custom")]
    Custom {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        custom_type: String,
        data: Option<Value>,
    },
    /// 自定义消息条目。
    #[serde(rename = "custom_message")]
    CustomMessage {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        custom_type: String,
        content: CustomMessageContent,
        details: Option<Value>,
        display: bool,
    },
    /// label 条目。
    #[serde(rename = "label")]
    Label {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        target_id: String,
        label: Option<String>,
    },
    /// 会话信息条目。
    #[serde(rename = "session_info")]
    SessionInfo {
        #[serde(flatten)]
        base: SessionTreeEntryBase,
        name: Option<String>,
    },
}

impl SessionTreeEntry {
    /// 返回条目公共基础字段。
    pub fn base(&self) -> &SessionTreeEntryBase {
        match self {
            SessionTreeEntry::Message { base, .. }
            | SessionTreeEntry::ThinkingLevelChange { base, .. }
            | SessionTreeEntry::ModelChange { base, .. }
            | SessionTreeEntry::Compaction { base, .. }
            | SessionTreeEntry::BranchSummary { base, .. }
            | SessionTreeEntry::Custom { base, .. }
            | SessionTreeEntry::CustomMessage { base, .. }
            | SessionTreeEntry::Label { base, .. }
            | SessionTreeEntry::SessionInfo { base, .. } => base,
        }
    }

    /// 返回条目 id。
    pub fn id(&self) -> &str {
        &self.base().id
    }

    /// 返回父条目 id。
    pub fn parent_id(&self) -> Option<&str> {
        self.base().parent_id.as_deref()
    }

    /// 返回条目关联的检查点提交 id。
    pub fn checkpoint_id(&self) -> Option<&str> {
        let (_, checkpoint_id) = self.id().rsplit_once('-')?;
        (checkpoint_id.len() == 40 && checkpoint_id.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then_some(checkpoint_id)
    }

    /// 返回条目类型字符串。
    pub fn entry_type(&self) -> &'static str {
        match self {
            SessionTreeEntry::Message { .. } => "message",
            SessionTreeEntry::ThinkingLevelChange { .. } => "thinking_level_change",
            SessionTreeEntry::ModelChange { .. } => "model_change",
            SessionTreeEntry::Compaction { .. } => "compaction",
            SessionTreeEntry::BranchSummary { .. } => "branch_summary",
            SessionTreeEntry::Custom { .. } => "custom",
            SessionTreeEntry::CustomMessage { .. } => "custom_message",
            SessionTreeEntry::Label { .. } => "label",
            SessionTreeEntry::SessionInfo { .. } => "session_info",
        }
    }
}

/// 自定义消息内容。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum CustomMessageContent {
    /// 纯文本。
    Text(String),
    /// 多模态内容块。
    Blocks(Vec<ContentBlock>),
}

/// 通过沿会话树某条路径重建出的对话上下文快照。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionContext {
    /// AgentMessage 序列。
    pub messages: Vec<AgentMessage>,
    /// 最近一次 thinking level。
    pub thinking_level: String,
    /// 最近一次 Model 选择。
    pub model: Option<SessionModelSelection>,
}

/// 会话中存储的模型选择。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelSelection {
    /// Provider 名称。
    pub provider: String,
    /// 模型 id。
    pub model_id: String,
}

/// 任意会话存储后端共享的元信息。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    /// 会话 id。
    pub id: String,
    /// 会话名称。
    pub name: String,
    /// 创建时间。
    pub created_at: String,
    /// 创建 cwd。
    pub cwd: String,
    /// 存储路径。
    pub path: String,
    /// fork 父会话路径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_path: Option<String>,
}

/// 创建底层会话存储时的通用选项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionCreateStorageOptions {
    /// 工作目录。
    pub cwd: String,
    /// 会话 id。
    pub session_id: String,
    /// 会话名称。
    pub name: String,
    /// 父会话路径。
    pub parent_session_path: Option<String>,
}

/// 从已有会话 fork 出新会话的选项。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionForkOptions {
    /// 锚点条目 id。
    pub entry_id: Option<String>,
    /// fork 位置。
    pub position: Option<SessionForkPosition>,
    /// 可选指定新 id。
    pub id: Option<String>,
}

/// fork 位置。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionForkPosition {
    /// 锚点之前。
    Before,
    /// 包含锚点。
    At,
}

/// 会话条目流与 leaf 锚点的存储协议。
#[async_trait]
pub trait SessionStorage: Send + Sync {
    /// 从指定路径打开已有会话。
    async fn open(path: &Path, session_id: &str) -> crate::agent::harness::HarnessResult<Arc<Self>>
    where
        Self: Sized;
    /// 创建新会话。
    async fn create(
        path: &Path,
        options: SessionCreateStorageOptions,
    ) -> crate::agent::harness::HarnessResult<Arc<Self>>
    where
        Self: Sized;
    /// 基于已加载数据构造存储实例。
    fn from_loaded(
        path: PathBuf,
        metadata: SessionMetadata,
        entries: Vec<SessionTreeEntry>,
        leaf_id: Option<String>,
    ) -> crate::agent::harness::HarnessResult<Arc<Self>>
    where
        Self: Sized;

    /// 返回会话元信息。
    async fn get_metadata(&self) -> SessionMetadata;
    /// 返回会话元信息读锁 guard。
    async fn with_metadata_guard(&self) -> RwLockReadGuard<'_, SessionMetadata>;
    /// 重命名会话。
    async fn rename(&self, name: String) -> crate::agent::harness::HarnessResult<()>;
    /// 返回当前 leaf id。
    async fn get_leaf_id(&self) -> Option<String>;
    /// 设置当前 leaf id。
    async fn set_leaf_id(&self, leaf_id: Option<String>) -> crate::agent::harness::HarnessResult<()>;
    /// 创建不冲突的新条目 id。
    async fn create_entry_id(&self) -> String;
    /// 追加条目。
    async fn append_entry(&self, entry: SessionTreeEntry) -> crate::agent::harness::HarnessResult<()>;
    /// 按 id 查询条目。
    async fn get_entry(&self, id: &str) -> Option<SessionTreeEntry>;
    /// 按类型查找条目。
    async fn find_entries(&self, entry_type: &str) -> Vec<SessionTreeEntry>;
    /// 返回 label。
    async fn get_label(&self, id: &str) -> Option<String>;
    /// 返回根到 leaf 的路径。
    async fn get_path_to_root(&self, leaf_id: Option<&str>) -> Vec<SessionTreeEntry>;
    /// 从 leaf 向 root 借用遍历路径条目；visitor 返回 `Break` 时停止遍历。
    async fn with_path_to_root(
        &self,
        leaf_id: Option<&str>,
        visitor: &mut (dyn for<'a> FnMut(&'a SessionTreeEntry) -> ControlFlow<()> + Send),
    );
    /// 返回全部条目。
    async fn get_entries(&self) -> Vec<SessionTreeEntry>;
    /// 借用全部条目执行只读访问。
    async fn with_entries(&self, visitor: &mut (dyn for<'a> FnMut(&'a [SessionTreeEntry]) + Send));
}

/// 会话仓库创建会话时使用的选项。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CreateOptions {
    /// 可选指定 id。
    pub id: Option<String>,
    /// 工作目录。
    pub cwd: String,
    /// 父会话路径。
    pub parent_session_path: Option<String>,
}

/// 会话仓库列举会话时的过滤选项。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ListOptions {
    /// 限定 cwd。
    pub cwd: Option<String>,
}

/// 会话仓库协议。
#[async_trait]
pub trait SessionRepo: Send + Sync {
    /// 返回供界面展示的仓储名称。
    fn name() -> &'static str
    where
        Self: Sized;

    /// 创建使用默认字段值的会话仓库。
    fn new() -> Self
    where
        Self: Sized;
    /// 使用会话仓储根路径初始化仓库。
    /// @param path 会话仓储根路径。
    async fn init(&self, path: PathBuf) -> crate::agent::harness::HarnessResult<()>;
    /// 创建会话。
    async fn create(
        &self,
        options: CreateOptions,
    ) -> crate::agent::harness::HarnessResult<crate::agent::harness::session::SessionHandle>;
    /// 打开会话。
    async fn open(
        &self,
        metadata: SessionMetadata,
    ) -> crate::agent::harness::HarnessResult<crate::agent::harness::session::SessionHandle>;
    /// fork 会话。
    async fn fork(
        &self,
        from_session: &crate::agent::harness::session::Session,
        options: SessionForkOptions,
    ) -> crate::agent::harness::HarnessResult<crate::agent::harness::session::SessionHandle>;
    /// 判断会话是否存在。
    async fn exists(&self, metadata: &SessionMetadata) -> bool;
    /// 重命名会话。
    async fn rename(&self, metadata: SessionMetadata, name: String) -> crate::agent::harness::HarnessResult<()>;
    /// 列举会话。
    async fn list(&self, options: ListOptions) -> crate::agent::harness::HarnessResult<Vec<SessionMetadata>>;
    /// 删除会话。
    async fn delete(&self, metadata: SessionMetadata) -> crate::agent::harness::HarnessResult<()>;
}

/// Harness 当前所处的阶段。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentHarnessPhase {
    /// 空闲。
    #[default]
    Idle,
    /// 正在处理一轮。
    Turn,
    /// 正在生成分支总结。
    BranchSummary,
    /// 正在重试。
    Retry,
}

/// Harness 自身事件。
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentHarnessOwnEvent<'a> {
    /// 队列更新。
    QueueUpdate { steer: &'a PendingMessageQueue, follow_up: &'a PendingMessageQueue, next_turn: &'a [AgentMessage] },
    /// 保存点。
    SavePoint { had_pending_mutations: bool },
    /// 中止事件。
    Abort { cleared_steer: &'a [AgentMessage], cleared_follow_up: &'a [AgentMessage] },
    /// settle 事件。
    Settled { next_turn_count: usize },
    /// 启动前事件。
    BeforeAgentStart {
        prompt: &'a str,
        images: &'a Option<Vec<ImageContent>>,
        system_prompt: &'a str,
        resources: &'a AgentHarnessResources,
    },
    /// context 事件。
    Context { messages: &'a [AgentMessage] },
    /// provider 请求前事件。
    BeforeProviderRequest { model: &'a Model, stream_options: &'a StreamOptions },
    /// payload 序列化前事件。
    BeforeProviderPayload { model: &'a Model, payload: &'a Value },
    /// provider 响应后事件。
    AfterProviderResponse { status: u16, headers: &'a HashMap<String, String> },
    /// tool call 前事件。
    ToolCall { tool_call_id: &'a str, tool_name: &'a str, input: &'a Map<String, Value> },
    /// tool result 前事件。
    ToolResult {
        tool_call_id: &'a str,
        tool_name: &'a str,
        input: &'a Map<String, Value>,
        content: &'a [ContentBlock],
        details: &'a Value,
        is_error: bool,
    },
    /// model 选择。
    ModelSelect { model: &'a Model, previous_model: Option<&'a Model>, source: ModelSelectSource },
    /// thinking level 选择。
    ThinkingLevelSelect { level: Option<&'a ThinkingLevel>, previous_level: Option<&'a ThinkingLevel> },
    /// 资源更新。
    ResourcesUpdate { resources: &'a AgentHarnessResources, previous_resources: &'a AgentHarnessResources },
}

/// Harness 对外暴露的事件联合类型。
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", content = "event")]
pub enum AgentHarnessEvent<'a> {
    /// 底层 AgentEvent。
    Agent(&'a AgentEvent<'a>),
    /// Harness 自身事件。
    Harness(&'a AgentHarnessOwnEvent<'a>),
}

/// Harness 事件监听器。
pub type AgentHarnessListener = dyn for<'a> MHook<AgentHarnessEvent<'a>, ()>;

/// `before_agent_start` 钩子。
pub type BeforeAgentStartHook = dyn for<'a> MHook<AgentHarnessOwnEvent<'a>, Option<BeforeAgentStartResult>>;

/// `context` 钩子。
pub type ContextHook = dyn MHook<Vec<AgentMessage>, Option<ContextResult>>;

/// Provider 请求前钩子。
pub type BeforeProviderRequestHook = dyn for<'a> MHook<AgentHarnessOwnEvent<'a>, Option<AgentHarnessStreamOptions>>;

/// Provider payload 序列化前钩子。
pub type BeforeProviderPayloadHook = dyn for<'a> MHook<AgentHarnessOwnEvent<'a>, Option<Value>>;

/// Provider 响应后钩子。
pub type AfterProviderResponseHook = dyn for<'a> MHook<AgentHarnessOwnEvent<'a>, ()>;

/// tool call 前钩子。
pub type ToolCallHook = dyn for<'a> MHook<AgentHarnessOwnEvent<'a>, Option<BeforeToolCallResult>>;

/// tool result 钩子。
pub type ToolResultHook = dyn for<'a> MHook<AgentHarnessOwnEvent<'a>, Option<AfterToolCallResult>>;

/// ModelSelect 来源。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelSelectSource {
    /// 外部 set。
    Set,
    /// 从会话恢复。
    Restore,
}

/// `before_agent_start` 钩子的返回值。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BeforeAgentStartResult {
    /// 替换 prompt。
    pub prompt: Option<String>,
    /// 插入到本轮初始消息前的额外消息。
    pub messages: Option<Vec<AgentMessage>>,
    /// 替换 system prompt。
    pub system_prompt: Option<String>,
    /// 替换图片。
    pub images: Option<Vec<ImageContent>>,
}

/// Context 钩子返回值。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ContextResult {
    /// 替换消息序列。
    pub messages: Option<Vec<AgentMessage>>,
}

/// Compaction 设置。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSettings {
    /// 是否启用。
    pub enabled: bool,
    /// 触发阈值 token（旧字段）。
    pub threshold_tokens: Option<u64>,
    /// 压缩后目标 token（旧字段）。
    pub target_tokens: Option<u64>,
    /// 预留给总结请求和后续上下文的 token 数。
    pub reserve_tokens: Option<u64>,
    /// 压缩时尽量保留最近多少 token。
    pub keep_recent_tokens: Option<u64>,
}

/// 压缩准备阶段计算出的元信息。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionPreparation {
    /// 是否需要压缩。
    pub should_compact: bool,
    /// token 估算。
    pub tokens_before: u64,
    /// 待总结消息。
    pub messages: Vec<AgentMessage>,
    /// 第一个保留 entry id。
    pub first_kept_entry_id: Option<String>,
}

/// 会话树切换准备信息。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TreePreparation {
    /// 旧 leaf。
    pub old_leaf_id: Option<String>,
    /// 新 leaf。
    pub new_leaf_id: Option<String>,
    /// 需要总结的分支条目。
    pub branch_entries: Vec<SessionTreeEntry>,
}

/// false 判断，用于 serde skip。
fn is_false(value: &bool) -> bool {
    !*value
}
