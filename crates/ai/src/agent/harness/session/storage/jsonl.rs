//! JSONL 文件版 `SessionStorage`：首行为会话元信息，后续每行是一条 `SessionTreeEntry`。

use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs,
    io::{AsyncBufReadExt, BufReader},
    sync::{RwLock, RwLockReadGuard},
};

use crate::agent::harness::{
    session::{
        session::now_iso,
        storage::memory::{build_labels_by_id, generate_entry_id, update_label_cache},
    },
    types::*,
    HarnessError, HarnessResult,
};

/// JSONL 文件版会话存储后端。
#[derive(Debug)]
pub struct JsonlSessionStorage {
    /// JSONL 文件绝对路径。
    file_path: PathBuf,
    /// 元信息。
    metadata: RwLock<SessionMetadata>,
    /// 内部可变状态。
    inner: RwLock<JsonlSessionStorageInner>,
}

/// JSONL 存储可变状态。
#[derive(Clone, Debug, Default)]
struct JsonlSessionStorageInner {
    /// 时间正序条目。
    entries: Vec<SessionTreeEntry>,
    /// id 索引。
    by_id: HashMap<String, SessionTreeEntry>,
    /// label 缓存。
    labels_by_id: HashMap<String, String>,
    /// 当前 leaf。
    current_leaf_id: Option<String>,
}

#[async_trait]
impl SessionStorage for JsonlSessionStorage {
    /// 从已存在的 JSONL 会话文件加载存储。
    async fn open(path: &Path, session_id: &str) -> HarnessResult<Arc<Self>> {
        let resolved = absolute_path(path)?.join(format!("{session_id}.jsonl"));
        let (metadata, entries, leaf_id) = load_jsonl_storage(&resolved).await?;
        Self::from_loaded(resolved, metadata, entries, leaf_id)
    }

    /// 创建一份全新的 JSONL 会话文件。
    async fn create(path: &Path, options: SessionCreateStorageOptions) -> HarnessResult<Arc<Self>> {
        let resolved = absolute_path(path)?;
        let metadata = SessionMetadata {
            id: options.session_id,
            name: options.name,
            created_at: now_iso(),
            cwd: options.cwd,
            path: resolved.display().to_string(),
            parent_session_path: options.parent_session_path,
        };
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&resolved, format!("{}\n", serde_json::to_string(&metadata)?)).await?;
        Self::from_loaded(resolved, metadata, Vec::new(), None)
    }

    /// 基于已加载数据构造存储实例。
    fn from_loaded(
        file_path: PathBuf,
        metadata: SessionMetadata,
        entries: Vec<SessionTreeEntry>,
        leaf_id: Option<String>,
    ) -> HarnessResult<Arc<Self>> {
        let by_id = entries.iter().cloned().map(|entry| (entry.id().to_string(), entry)).collect::<HashMap<_, _>>();
        if let Some(leaf_id) = &leaf_id {
            if !by_id.contains_key(leaf_id) {
                return Err(HarnessError::Message(format!("Entry {leaf_id} not found")));
            }
        }
        let labels_by_id = build_labels_by_id(&entries);
        Ok(Arc::new(Self {
            file_path,
            metadata: RwLock::new(metadata),
            inner: RwLock::new(JsonlSessionStorageInner { entries, by_id, labels_by_id, current_leaf_id: leaf_id }),
        }))
    }

    /// 返回会话元信息。
    async fn get_metadata(&self) -> SessionMetadata {
        self.metadata.read().await.clone()
    }

    /// 返回会话元信息读锁 guard。
    async fn with_metadata_guard(&self) -> RwLockReadGuard<'_, SessionMetadata> {
        self.metadata.read().await
    }

    /// 重命名 JSONL 会话。
    async fn rename(&self, name: String) -> HarnessResult<()> {
        let mut metadata = self.metadata.write().await;
        metadata.name = name;
        rewrite_jsonl_session_metadata(&self.file_path, &metadata).await?;
        Ok(())
    }

    /// 返回当前 leaf。
    async fn get_leaf_id(&self) -> Option<String> {
        self.inner.read().await.current_leaf_id.clone()
    }

    /// 设置当前 leaf。
    async fn set_leaf_id(&self, leaf_id: Option<String>) -> HarnessResult<()> {
        let mut inner = self.inner.write().await;
        if let Some(leaf_id) = &leaf_id {
            if !inner.by_id.contains_key(leaf_id) {
                return Err(HarnessError::Message(format!("Entry {leaf_id} not found")));
            }
        }
        inner.current_leaf_id = leaf_id;
        Ok(())
    }

    /// 创建新条目 id。
    async fn create_entry_id(&self) -> String {
        generate_entry_id(&self.inner.read().await.by_id)
    }

    /// 追加条目。
    async fn append_entry(&self, entry: SessionTreeEntry) -> HarnessResult<()> {
        let serialized = serde_json::to_string(&entry)?;
        use tokio::io::AsyncWriteExt;
        let mut file = fs::OpenOptions::new().append(true).create(true).open(&self.file_path).await?;
        file.write_all(serialized.as_bytes()).await?;
        file.write_all(b"\n").await?;
        let mut inner = self.inner.write().await;
        update_label_cache(&mut inner.labels_by_id, &entry);
        inner.current_leaf_id = Some(entry.id().to_string());
        inner.by_id.insert(entry.id().to_string(), entry.clone());
        inner.entries.push(entry);
        Ok(())
    }

    /// 按 id 查询条目。
    async fn get_entry(&self, id: &str) -> Option<SessionTreeEntry> {
        self.inner.read().await.by_id.get(id).cloned()
    }

    /// 按类型筛选条目。
    async fn find_entries(&self, entry_type: &str) -> Vec<SessionTreeEntry> {
        self.inner.read().await.entries.iter().filter(|entry| entry.entry_type() == entry_type).cloned().collect()
    }

    /// 返回 label。
    async fn get_label(&self, id: &str) -> Option<String> {
        self.inner.read().await.labels_by_id.get(id).cloned()
    }

    /// 返回 root 到 leaf 路径。
    async fn get_path_to_root(&self, leaf_id: Option<&str>) -> Vec<SessionTreeEntry> {
        let Some(leaf_id) = leaf_id else { return Vec::new() };
        let inner = self.inner.read().await;
        let mut path = Vec::new();
        let mut current = inner.by_id.get(leaf_id).cloned();
        while let Some(entry) = current {
            current = entry.parent_id().and_then(|parent_id| inner.by_id.get(parent_id).cloned());
            path.insert(0, entry);
        }
        path
    }

    /// 从 leaf 向 root 借用遍历路径条目。
    async fn with_path_to_root(
        &self,
        leaf_id: Option<&str>,
        visitor: &mut (dyn for<'a> FnMut(&'a SessionTreeEntry) -> std::ops::ControlFlow<()> + Send),
    ) {
        let Some(leaf_id) = leaf_id else { return };
        let inner = self.inner.read().await;
        let mut current = inner.by_id.get(leaf_id);
        while let Some(entry) = current {
            if visitor(entry).is_break() {
                break;
            }
            current = entry.parent_id().and_then(|parent_id| inner.by_id.get(parent_id));
        }
    }

    /// 返回全部条目。
    async fn get_entries(&self) -> Vec<SessionTreeEntry> {
        self.inner.read().await.entries.clone()
    }

    /// 借用全部条目执行只读访问。
    async fn with_entries(&self, visitor: &mut (dyn for<'a> FnMut(&'a [SessionTreeEntry]) + Send)) {
        let inner = self.inner.read().await;
        visitor(&inner.entries);
    }
}

/// 重写 JSONL 文件首行元信息。
async fn rewrite_jsonl_session_metadata(file_path: &Path, metadata: &SessionMetadata) -> HarnessResult<()> {
    let content = fs::read_to_string(file_path).await?;
    let mut lines = content.lines();
    let Some(_header) = lines.next() else {
        return Err(HarnessError::Message(format!(
            "Invalid JSONL session file {}: missing session header",
            file_path.display()
        )));
    };
    let mut updated = serde_json::to_string(metadata)?;
    updated.push('\n');
    for line in lines {
        updated.push_str(line);
        updated.push('\n');
    }
    fs::write(file_path, updated).await?;
    Ok(())
}

/// 读取 JSONL 文件首行元信息。
pub async fn load_jsonl_session_metadata(file_path: impl AsRef<Path>) -> HarnessResult<SessionMetadata> {
    let resolved = absolute_path(file_path.as_ref())?;
    let file = fs::File::open(&resolved).await?;
    let mut lines = BufReader::new(file).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            break;
        }
        let mut metadata: SessionMetadata = serde_json::from_str(&line)?;
        metadata.path = resolved.display().to_string();
        return Ok(metadata);
    }
    Err(HarnessError::Message(format!("Invalid JSONL session file {}: missing session header", resolved.display())))
}

/// 全量加载 JSONL 存储。
async fn load_jsonl_storage(
    file_path: &Path,
) -> HarnessResult<(SessionMetadata, Vec<SessionTreeEntry>, Option<String>)> {
    let content = fs::read_to_string(file_path).await?;
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let Some(header_line) = lines.next() else {
        return Err(HarnessError::Message(format!(
            "Invalid JSONL session file {}: missing session header",
            file_path.display()
        )));
    };
    let mut metadata: SessionMetadata = serde_json::from_str(header_line)?;
    metadata.path = file_path.display().to_string();
    let mut entries = Vec::new();
    let mut leaf_id = None;
    for line in lines {
        if let Ok(entry) = serde_json::from_str::<SessionTreeEntry>(line) {
            leaf_id = Some(entry.id().to_string());
            entries.push(entry);
        }
    }
    Ok((metadata, entries, leaf_id))
}

/// 获取绝对路径。
fn absolute_path(path: &Path) -> std::io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
