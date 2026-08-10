//! 进程内会话仓库实现，用于测试与无持久化场景。

use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

use crate::agent::harness::{
    session::{
        repo::shared::{create_session_id, create_timestamp, get_entries_to_fork},
        storage::memory::InMemorySessionStorage,
        Session, SessionHandle,
    },
    types::*,
    HarnessError, HarnessResult,
};

/// 内存会话仓库。
#[derive(Debug, Default)]
pub struct InMemorySessionRepo {
    /// 已创建的 session storage。
    sessions: RwLock<HashMap<String, Arc<InMemorySessionStorage>>>,
}

#[async_trait]
impl SessionRepo for InMemorySessionRepo {
    /// 返回供界面展示的仓储名称。
    fn name() -> &'static str {
        "Memory"
    }

    /// 创建空仓库。
    fn new() -> Self {
        Self { sessions: RwLock::new(HashMap::new()) }
    }

    /// 初始化内存仓库。
    async fn init(&self, _path: PathBuf) -> HarnessResult<()> {
        Ok(())
    }

    /// 创建会话。
    async fn create(&self, options: CreateOptions) -> HarnessResult<SessionHandle> {
        let id = options.id.unwrap_or_else(create_session_id);
        let storage = InMemorySessionStorage::create(
            Path::new(""),
            SessionCreateStorageOptions {
                cwd: options.cwd,
                session_id: id.clone(),
                name: String::new(),
                parent_session_path: options.parent_session_path,
            },
        )
        .await?;
        self.sessions.write().await.insert(id, Arc::clone(&storage));
        Ok(Arc::new(Session::new(storage)))
    }

    /// 打开会话。
    async fn open(&self, metadata: SessionMetadata) -> HarnessResult<SessionHandle> {
        let session_id = metadata.id;
        let sessions = self.sessions.read().await;
        let Some(storage) = sessions.get(&session_id).cloned() else {
            return Err(HarnessError::Message(format!("Session {session_id} not found")));
        };
        Ok(Arc::new(Session::new(storage)))
    }

    /// fork 一个内存会话。
    async fn fork(&self, from_session: &Session, options: SessionForkOptions) -> HarnessResult<SessionHandle> {
        let entries = get_entries_to_fork(from_session, &options).await;
        let source_metadata = from_session.get_metadata().await;
        let id = options.id.unwrap_or_else(create_session_id);
        let metadata = SessionMetadata {
            id: id.clone(),
            name: format!("fork:{}", source_metadata.name),
            created_at: create_timestamp(),
            cwd: source_metadata.cwd,
            path: String::new(),
            parent_session_path: Some(source_metadata.path),
        };
        let storage = InMemorySessionStorage::from_loaded(PathBuf::new(), metadata, entries, None)?;
        self.sessions.write().await.insert(id, Arc::clone(&storage));
        Ok(Arc::new(Session::new(storage)))
    }

    /// 判断内存会话是否存在。
    async fn exists(&self, _metadata: &SessionMetadata) -> bool {
        false
    }

    /// 重命名内存会话。
    async fn rename(&self, metadata: SessionMetadata, name: String) -> HarnessResult<()> {
        let sessions = self.sessions.read().await;
        let Some(storage) = sessions.get(&metadata.id) else {
            return Err(HarnessError::Message(format!("Session {} not found", metadata.id)));
        };
        storage.rename(name).await
    }

    /// 列举会话。
    async fn list(&self, _options: ListOptions) -> HarnessResult<Vec<SessionMetadata>> {
        let sessions = self.sessions.read().await;
        let mut metadatas = Vec::new();
        for storage in sessions.values() {
            metadatas.push(storage.get_metadata().await);
        }
        metadatas.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(metadatas)
    }

    /// 删除会话。
    async fn delete(&self, metadata: SessionMetadata) -> HarnessResult<()> {
        self.sessions.write().await.remove(&metadata.id);
        Ok(())
    }
}
