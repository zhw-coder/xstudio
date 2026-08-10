use ai::{
    agent::harness::{AgentHarnessStreamOptions, SessionMetadata},
    model::{ImageContent, Model, ThinkingLevel},
};
use serde::{Deserialize, Serialize};

/// 会话列表请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatListSessionsInput {
    /// 会话仓储名称。
    pub storage_type: String,
}

/// 打开会话请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatOpenSessionInput {
    /// 会话仓储名称。
    pub storage_type: String,
    /// 会话元信息。
    pub metadata: SessionMetadata,
}

/// 创建会话请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCreateSessionInput {
    /// 会话仓储名称。
    pub storage_type: String,
    /// 当前会话使用的 AI 模型。
    pub model: Model,
    /// 当前会话使用的模型思考等级；None 表示 off。
    pub thinking_level: Option<ThinkingLevel>,
}

/// 基于聊天消息创建独立会话的请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatForkSessionInput {
    /// 会话仓储名称。
    pub storage_type: String,
    /// 源会话 id。
    pub source_session_id: String,
    /// 新会话保留到该聊天消息之前的倒序索引，`0` 表示最新消息。
    pub index: usize,
}

/// 删除会话请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDeleteSessionInput {
    /// 会话仓储名称。
    pub storage_type: String,
    /// 会话元信息。
    pub metadata: SessionMetadata,
}

/// 对会话发起 prompt 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPromptInput {
    /// 会话 id。
    pub session_id: String,
    /// 用户输入文本。
    pub text: String,
    /// 用户输入图像。
    pub images: Option<Vec<ImageContent>>,
}

/// 终止会话当前 run 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAbortInput {
    /// 会话 id。
    pub session_id: String,
}

/// 工具审批结算请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResolveToolApprovalInput {
    /// 会话 id。
    pub session_id: String,
    /// 审批请求 id。
    pub approval_id: String,
    /// 是否允许工具执行。
    pub approved: bool,
}

/// 查询已缓存会话 Harness 资源名称请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatListResourceNamesInput {
    /// 会话 id。
    pub session_id: String,
}

/// 可由聊天命令调用的资源摘要。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatResourceNameOutput {
    /// 资源稳定名称。
    pub name: String,
    /// 资源简短描述。
    pub description: String,
}

/// 压缩会话历史请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompactInput {
    /// 会话 id。
    pub session_id: String,
    /// 可选的自定义摘要指令。
    pub custom_instructions: Option<String>,
}

/// 回撤会话中一条用户消息及其后续内容的请求。
///
/// 回撤会先中止运行中的 run 并等待待写入内容落盘，再切换活跃分支；不会删除原有条目，原分支可通过树导航恢复。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWithdrawTurnInput {
    /// 会话 id。
    pub session_id: String,
    /// 要回撤的聊天消息倒序索引，`0` 表示最新消息。
    pub index: usize,
}

/// 在当前会话中回撤用户消息以供客户端编辑后重新发送的请求。
///
/// 请求会中止正在执行的 run，并回撤到被编辑消息之前的分支。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEditAndPromptUserMessageInput {
    /// 会话 id。
    pub session_id: String,
    /// 被编辑替换的聊天消息倒序索引，`0` 表示最新消息。
    pub index: usize,
}

/// 对会话发起 skill 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSkillInput {
    /// 会话 id。
    pub session_id: String,
    /// Skill 名称。
    pub name: String,
    /// 额外指令。
    pub additional_instructions: Option<String>,
}

/// 对会话发起 prompt template 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTemplateInput {
    /// 会话 id。
    pub session_id: String,
    /// PromptTemplate 名称。
    pub name: String,
    /// 模板参数。
    pub args: Vec<String>,
}

/// 更新会话 stream options 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSetStreamOptionsInput {
    /// 会话 id。
    pub session_id: String,
    /// Provider 请求选项。
    pub stream_options: AgentHarnessStreamOptions,
}

/// 更新会话模型请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSetModelInput {
    /// 会话 id。
    pub session_id: String,
    /// 当前会话使用的 AI 模型。
    pub model: Model,
}

/// 更新会话 thinking level 请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSetThinkingLevelInput {
    /// 会话 id。
    pub session_id: String,
    /// 模型思考等级；None 表示 off。
    pub thinking_level: Option<ThinkingLevel>,
}

/// 更新会话激活工具请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSetToolsInput {
    /// 会话 id。
    pub session_id: String,
    /// 工具配置数组，首元素为启用状态（0/1），其余元素为工具名称。
    pub tools: Vec<String>,
}

/// 更新会话名称请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSetSessionNameInput {
    /// 会话 id。
    pub session_id: String,
    /// 会话名称。
    pub name: String,
}
