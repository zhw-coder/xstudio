//! 按 cwd 分组的 JSONL 会话仓库实现。

use async_trait::async_trait;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{fs, sync::RwLock};

use crate::agent::harness::{
    session::{
        repo::shared::{create_session_id, get_entries_to_fork},
        storage::jsonl::{load_jsonl_session_metadata, JsonlSessionStorage},
        Session, SessionHandle,
    },
    types::*,
    HarnessError, HarnessResult,
};

/// JSONL 会话仓库。
#[derive(Clone, Debug)]
pub struct JsonlSessionRepo {
    /// 会话根目录。
    root_dir: Arc<RwLock<PathBuf>>,
}

#[async_trait]
impl SessionRepo for JsonlSessionRepo {
    /// 返回供界面展示的仓储名称。
    fn name() -> &'static str {
        "Jsonl"
    }

    /// 创建使用默认根目录的 JSONL 会话仓库。
    fn new() -> Self {
        Self { root_dir: Arc::new(RwLock::new(PathBuf::new())) }
    }

    /// 设置 JSONL 会话仓库根目录。
    async fn init(&self, path: PathBuf) -> HarnessResult<()> {
        *self.root_dir.write().await = path;
        Ok(())
    }

    /// 创建会话。
    async fn create(&self, options: CreateOptions) -> HarnessResult<SessionHandle> {
        let session_id = options.id.unwrap_or_else(create_session_id);
        let root_dir = self.root_dir.read().await;
        let file_path = root_dir.join(encode_cwd(&options.cwd)).join(format!("{session_id}.jsonl"));
        let storage = JsonlSessionStorage::create(
            &file_path,
            SessionCreateStorageOptions {
                cwd: options.cwd,
                session_id,
                name: String::new(),
                parent_session_path: options.parent_session_path,
            },
        )
        .await?;
        drop(root_dir);
        Ok(Arc::new(Session::new(storage)))
    }

    /// 打开会话。
    async fn open(&self, metadata: SessionMetadata) -> HarnessResult<SessionHandle> {
        if !Path::new(&metadata.path).exists() {
            return Err(HarnessError::Message(format!("Session file {} not found", metadata.path)));
        }
        let parent = Path::new(&metadata.path).parent().unwrap_or_else(|| Path::new("."));
        let storage = JsonlSessionStorage::open(parent, &metadata.id).await?;
        Ok(Arc::new(Session::new(storage)))
    }

    /// fork 一个 JSONL 会话。
    async fn fork(&self, from_session: &Session, options: SessionForkOptions) -> HarnessResult<SessionHandle> {
        let metadata = from_session.with_metadata_guard().await;
        let (cwd, parent_session_path, source_name) =
            (metadata.cwd.to_string(), metadata.path.to_string(), metadata.name.to_string());
        let entries = get_entries_to_fork(from_session, &options).await;
        let id = options.id.unwrap_or_else(create_session_id);
        let root_dir = self.root_dir.read().await;
        let path = root_dir.join(encode_cwd(&cwd)).join(format!("{id}.jsonl"));
        let storage = JsonlSessionStorage::create(
            &path,
            SessionCreateStorageOptions {
                cwd,
                session_id: id,
                name: format!("fork:{source_name}"),
                parent_session_path: Some(parent_session_path),
            },
        )
        .await?;
        for entry in entries {
            storage.append_entry(entry).await?;
        }
        Ok(Arc::new(Session::new(storage)))
    }

    /// 判断会话文件是否存在。
    async fn exists(&self, metadata: &SessionMetadata) -> bool {
        fs::metadata(&metadata.path).await.is_ok()
    }

    /// 重命名 JSONL 会话。
    async fn rename(&self, metadata: SessionMetadata, name: String) -> HarnessResult<()> {
        let session = self.open(metadata).await?;
        session.get_storage().rename(name).await
    }

    /// 列举会话。
    async fn list(&self, options: ListOptions) -> HarnessResult<Vec<SessionMetadata>> {
        let mut results = Vec::new();
        let root_dir = self.root_dir.read().await;
        let dirs = if let Some(cwd) = options.cwd {
            vec![root_dir.join(encode_cwd(&cwd))]
        } else {
            let mut dirs = Vec::new();
            if let Ok(mut entries) = fs::read_dir(&*root_dir).await {
                while let Some(entry) = entries.next_entry().await? {
                    if entry.file_type().await?.is_dir() {
                        dirs.push(entry.path());
                    }
                }
            }
            dirs
        };
        for dir in dirs {
            if let Ok(mut entries) = fs::read_dir(dir).await {
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
                        if let Ok(metadata) = load_jsonl_session_metadata(&path).await {
                            results.push(metadata);
                        }
                    }
                }
            }
        }
        results.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        drop(root_dir);
        Ok(results)
    }

    /// 删除会话。
    async fn delete(&self, metadata: SessionMetadata) -> HarnessResult<()> {
        if fs::metadata(&metadata.path).await.is_ok() {
            fs::remove_file(metadata.path).await?;
        }
        Ok(())
    }
}

/// 将 cwd 编码为安全目录名。
pub fn encode_cwd(cwd: &str) -> String {
    cwd.bytes().map(|byte| format!("{byte:02x}")).collect()
}
