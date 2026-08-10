//! 把 `SessionStorage` 抽象出的条目流与 leaf 锚点协议封装成 Harness 使用的高层会话 API。

use serde_json::Value;
use std::{ops::ControlFlow, sync::Arc};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::RwLockReadGuard;

use crate::{
    agent::{
        harness::{
            messages::{
                create_branch_summary_message, create_custom_message, COMPACTION_SUMMARY_PREFIX,
                COMPACTION_SUMMARY_SUFFIX,
            },
            types::*,
        },
        types::AgentMessage,
    },
    model::types::{ContentBlock, TextContent, UserContent, UserMessage},
};

/// 类型擦除后的 SessionStorage 句柄。
pub type StorageHandle = Arc<dyn SessionStorage>;

/// 类型擦除后的 Session 句柄。
pub type SessionHandle = Arc<Session>;

/// 把会话树某条路径上的条目序列重建为对话上下文快照。
pub fn build_session_context(path_entries: &[SessionTreeEntry]) -> SessionContext {
    let mut thinking_level = "off".to_string();
    let mut model: Option<SessionModelSelection> = None;
    let mut compaction_idx: Option<usize> = None;
    for (index, entry) in path_entries.iter().enumerate() {
        match entry {
            SessionTreeEntry::ThinkingLevelChange { thinking_level: level, .. } => thinking_level = level.clone(),
            SessionTreeEntry::ModelChange { provider, model_id, .. } => {
                model = Some(SessionModelSelection { provider: provider.clone(), model_id: model_id.clone() });
            }
            SessionTreeEntry::Message { message: AgentMessage::Assistant(message), .. } => {
                model =
                    Some(SessionModelSelection { provider: message.provider.clone(), model_id: message.model.clone() });
            }
            SessionTreeEntry::Compaction { .. } => compaction_idx = Some(index),
            _ => {}
        }
    }
    let mut messages = Vec::new();
    if let Some(compaction_idx) = compaction_idx {
        // 上下文压缩路径：先加入压缩 summary，再保留压缩条目前从 `first_kept_entry_id` 开始的条目，
        // 最后追加压缩条目之后的全部条目。
        let SessionTreeEntry::Compaction { base, summary, first_kept_entry_id, .. } = &path_entries[compaction_idx]
        else {
            unreachable!("compaction_idx must point to compaction entry");
        };
        messages.push(AgentMessage::User(UserMessage {
            content: UserContent::Blocks(vec![ContentBlock::Text(TextContent {
                text: format!("{COMPACTION_SUMMARY_PREFIX}{summary}{COMPACTION_SUMMARY_SUFFIX}"),
                text_signature: None,
            })]),
            timestamp: timestamp_ms(&base.timestamp),
        }));
        let mut found_first_kept = false;
        for entry in path_entries.iter().take(compaction_idx) {
            if entry.id() == first_kept_entry_id {
                found_first_kept = true;
            }
            if found_first_kept {
                append_context_message(&mut messages, entry);
            }
        }
        for entry in path_entries.iter().skip(compaction_idx + 1) {
            append_context_message(&mut messages, entry);
        }
    } else {
        for entry in path_entries {
            append_context_message(&mut messages, entry);
        }
    }
    SessionContext { messages, thinking_level, model }
}

/// 把会话树某条路径上的条目序列重建为展示用上下文快照。
pub fn build_session_context_view(path_entries: &[SessionTreeEntry]) -> SessionContext {
    let mut thinking_level = "off".to_string();
    let mut model: Option<SessionModelSelection> = None;
    let mut messages = Vec::new();
    for entry in path_entries {
        match entry {
            SessionTreeEntry::ThinkingLevelChange { thinking_level: level, .. } => thinking_level = level.clone(),
            SessionTreeEntry::ModelChange { provider, model_id, .. } => {
                model = Some(SessionModelSelection { provider: provider.clone(), model_id: model_id.clone() });
            }
            SessionTreeEntry::Message { message, .. } => {
                if let AgentMessage::Assistant(message) = message {
                    model = Some(SessionModelSelection {
                        provider: message.provider.clone(),
                        model_id: message.model.clone(),
                    });
                }
                messages.push(message.clone());
            }
            _ => {}
        }
    }
    SessionContext { messages, thinking_level, model }
}

/// 会话管理类。
pub struct Session {
    /// 底层存储后端。
    storage: StorageHandle,
}

impl Session {
    /// 构造一个 Session。
    pub fn new(storage: StorageHandle) -> Self {
        Self { storage }
    }

    /// 返回当前会话元信息。
    pub async fn get_metadata(&self) -> SessionMetadata {
        self.storage.get_metadata().await
    }

    /// 返回当前会话元信息读锁 guard。
    pub async fn with_metadata_guard(&self) -> RwLockReadGuard<'_, SessionMetadata> {
        self.storage.with_metadata_guard().await
    }

    /// 返回底层存储后端。
    pub fn get_storage(&self) -> &StorageHandle {
        &self.storage
    }

    /// 返回当前 leaf id。
    pub async fn get_leaf_id(&self) -> Option<String> {
        self.storage.get_leaf_id().await
    }

    /// 按 id 查询条目。
    pub async fn get_entry(&self, id: &str) -> Option<SessionTreeEntry> {
        self.storage.get_entry(id).await
    }

    /// 按倒序索引返回带检查点后缀绑定的聊天 message 条目。
    ///
    /// @param index 聊天 message 的倒序索引，`0` 表示最新条目。
    pub async fn get_chat_entry(&self, mut index: usize) -> Option<SessionTreeEntry> {
        let mut entry = None;
        let leaf_id = self.storage.get_leaf_id().await;
        self.storage
            .with_path_to_root(leaf_id.as_deref(), &mut |candidate| {
                if !matches!(candidate, SessionTreeEntry::Message { .. }) || candidate.checkpoint_id().is_none() {
                    return ControlFlow::Continue(());
                }
                if index == 0 {
                    entry = Some(candidate.clone());
                    return ControlFlow::Break(());
                }
                index -= 1;
                ControlFlow::Continue(())
            })
            .await;
        entry
    }

    /// 返回全部条目。
    pub async fn get_entries(&self) -> Vec<SessionTreeEntry> {
        self.storage.get_entries().await
    }

    /// 借用全部条目执行只读访问。
    pub async fn with_entries(&self, mut visitor: impl FnMut(&[SessionTreeEntry]) + Send) {
        self.storage.with_entries(&mut visitor).await;
    }

    /// 返回指定 leaf 到 root 的路径。
    pub async fn get_branch(&self, from_id: Option<&str>) -> Vec<SessionTreeEntry> {
        let leaf_id = match from_id {
            Some(id) => Some(id.to_string()),
            None => self.storage.get_leaf_id().await,
        };
        self.storage.get_path_to_root(leaf_id.as_deref()).await
    }

    /// 沿当前分支重建对话上下文。
    pub async fn build_context(&self) -> SessionContext {
        build_session_context(&self.get_branch(None).await)
    }

    /// 沿当前分支重建展示用对话上下文。
    pub async fn build_context_view(&self) -> SessionContext {
        build_session_context_view(&self.get_branch(None).await)
    }

    /// 返回某条目当前生效的 label。
    pub async fn get_label(&self, id: &str) -> Option<String> {
        self.storage.get_label(id).await
    }

    /// 返回当前会话名称。
    pub async fn get_session_name(&self) -> Option<String> {
        self.storage.find_entries("session_info").await.into_iter().rev().find_map(|entry| match entry {
            SessionTreeEntry::SessionInfo { name, .. } => {
                name.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
            }
            _ => None,
        })
    }

    /// 把一条 AgentMessage 追加为 `message` 条目。
    ///
    /// @param commit_id 触发模型请求的 user 消息对应的检查点 id。
    pub async fn append_message(
        &self,
        message: AgentMessage,
        commit_id: Option<&str>,
    ) -> crate::agent::harness::HarnessResult<String> {
        let entry = SessionTreeEntry::Message { base: self.new_base(commit_id).await, message };
        self.append_typed_entry(entry).await
    }

    /// 追加 thinking level change 条目。
    pub async fn append_thinking_level_change(
        &self,
        thinking_level: String,
    ) -> crate::agent::harness::HarnessResult<String> {
        let entry = SessionTreeEntry::ThinkingLevelChange { base: self.new_base(None).await, thinking_level };
        self.append_typed_entry(entry).await
    }

    /// 追加 model change 条目。
    pub async fn append_model_change(
        &self,
        provider: String,
        model_id: String,
    ) -> crate::agent::harness::HarnessResult<String> {
        let entry = SessionTreeEntry::ModelChange { base: self.new_base(None).await, provider, model_id };
        self.append_typed_entry(entry).await
    }

    /// 追加 compaction 条目。
    pub async fn append_compaction(
        &self,
        summary: String,
        first_kept_entry_id: String,
        tokens_before: u64,
        details: Option<Value>,
        from_hook: Option<bool>,
    ) -> crate::agent::harness::HarnessResult<String> {
        let entry = SessionTreeEntry::Compaction {
            base: self.new_base(None).await,
            summary,
            first_kept_entry_id,
            tokens_before,
            details,
            from_hook,
        };
        self.append_typed_entry(entry).await
    }

    /// 追加 custom 条目。
    pub async fn append_custom_entry(
        &self,
        custom_type: String,
        data: Option<Value>,
    ) -> crate::agent::harness::HarnessResult<String> {
        let entry = SessionTreeEntry::Custom { base: self.new_base(None).await, custom_type, data };
        self.append_typed_entry(entry).await
    }

    /// 追加 custom_message 条目。
    pub async fn append_custom_message_entry(
        &self,
        custom_type: String,
        content: CustomMessageContent,
        display: bool,
        details: Option<Value>,
    ) -> crate::agent::harness::HarnessResult<String> {
        let entry =
            SessionTreeEntry::CustomMessage { base: self.new_base(None).await, custom_type, content, display, details };
        self.append_typed_entry(entry).await
    }

    /// 追加 label 条目。
    pub async fn append_label(
        &self,
        target_id: String,
        label: Option<String>,
    ) -> crate::agent::harness::HarnessResult<String> {
        if self.storage.get_entry(&target_id).await.is_none() {
            return Err(crate::agent::harness::HarnessError::Message(format!("Entry {target_id} not found")));
        }
        let entry = SessionTreeEntry::Label { base: self.new_base(None).await, target_id, label };
        self.append_typed_entry(entry).await
    }

    /// 追加 session_info 条目。
    pub async fn append_session_name(&self, name: String) -> crate::agent::harness::HarnessResult<String> {
        let name = name.trim().to_string();
        self.storage.rename(name.clone()).await?;
        let entry = SessionTreeEntry::SessionInfo { base: self.new_base(None).await, name: Some(name) };
        self.append_typed_entry(entry).await
    }

    /// 切换 leaf，并可选追加 branch summary。
    pub async fn move_to(
        &self,
        entry_id: Option<String>,
        summary: Option<BranchMoveSummary>,
    ) -> crate::agent::harness::HarnessResult<Option<String>> {
        if let Some(entry_id) = &entry_id {
            if self.storage.get_entry(entry_id).await.is_none() {
                return Err(crate::agent::harness::HarnessError::Message(format!("Entry {entry_id} not found")));
            }
        }
        self.storage.set_leaf_id(entry_id.clone()).await?;
        let Some(summary) = summary else { return Ok(None) };
        let from_id = entry_id.as_deref().unwrap_or("root").to_string();
        let entry = SessionTreeEntry::BranchSummary {
            base: SessionTreeEntryBase {
                id: self.storage.create_entry_id().await,
                parent_id: entry_id,
                timestamp: now_iso(),
            },
            from_id,
            summary: summary.summary,
            details: summary.details,
            from_hook: summary.from_hook,
        };
        self.append_typed_entry(entry).await.map(Some)
    }

    /// 创建条目公共字段。
    ///
    /// @param commit_id 可选的检查点 id。
    async fn new_base(&self, commit_id: Option<&str>) -> SessionTreeEntryBase {
        let id = self.storage.create_entry_id().await;
        SessionTreeEntryBase {
            id: commit_id.map(|commit_id| format!("{id}-{commit_id}")).unwrap_or(id),
            parent_id: self.storage.get_leaf_id().await,
            timestamp: now_iso(),
        }
    }

    /// 统一追加条目。
    async fn append_typed_entry(&self, entry: SessionTreeEntry) -> crate::agent::harness::HarnessResult<String> {
        let id = entry.id().to_string();
        self.storage.append_entry(entry).await?;
        Ok(id)
    }
}

/// moveTo 时可选的分支总结。
#[derive(Clone, Debug, Default)]
pub struct BranchMoveSummary {
    /// 总结正文。
    pub summary: String,
    /// details 负载。
    pub details: Option<Value>,
    /// 是否由钩子提供。
    pub from_hook: Option<bool>,
}

/// 把一条会话条目转换并追加到 AgentMessage 序列。
fn append_context_message(messages: &mut Vec<AgentMessage>, entry: &SessionTreeEntry) {
    match entry {
        SessionTreeEntry::Message { message, .. } => messages.push(message.clone()),
        SessionTreeEntry::CustomMessage { base, custom_type, content, display, details } => {
            let custom = create_custom_message(
                custom_type.clone(),
                content.clone(),
                *display,
                details.clone(),
                timestamp_ms(&base.timestamp),
            );
            messages.push(AgentMessage::Custom {
                kind: "custom".to_string(),
                payload: serde_json::to_value(custom).unwrap_or(Value::Null),
            });
        }
        SessionTreeEntry::BranchSummary { base, summary, from_id, .. } if !summary.is_empty() => {
            let branch = create_branch_summary_message(summary.clone(), from_id.clone(), timestamp_ms(&base.timestamp));
            messages.push(AgentMessage::Custom {
                kind: "branchSummary".to_string(),
                payload: serde_json::to_value(branch).unwrap_or(Value::Null),
            });
        }
        _ => {}
    }
}

/// 生成当前 ISO 8601 时间字符串。
pub fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp().to_string())
}

/// 把 ISO 时间字符串转换为毫秒时间戳。
fn timestamp_ms(timestamp: &str) -> i64 {
    OffsetDateTime::parse(timestamp, &Rfc3339)
        .map(|value| value.unix_timestamp_nanos() as i64 / 1_000_000)
        .unwrap_or_default()
}

/// 将内容块过滤为 Session 自定义消息可接受的块。
#[allow(dead_code)]
fn custom_content_blocks(blocks: Vec<ContentBlock>) -> CustomMessageContent {
    CustomMessageContent::Blocks(blocks)
}
