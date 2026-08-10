//! 内存版 `SessionStorage` 实现，把会话条目存放在进程内的数组与 HashMap 中。

use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{RwLock, RwLockReadGuard};
use uuid::Uuid;

use crate::agent::harness::{session::session::now_iso, types::*, HarnessError, HarnessResult};

/// 内存版会话存储后端。
#[derive(Debug)]
pub struct InMemorySessionStorage {
    /// 内部可变状态。
    inner: RwLock<InMemorySessionStorageInner>,
    /// 会话元信息。
    metadata: RwLock<SessionMetadata>,
}

/// 内存存储可变状态。
#[derive(Clone, Debug, Default)]
struct InMemorySessionStorageInner {
    /// 时间正序条目。
    entries: Vec<SessionTreeEntry>,
    /// id 索引。
    by_id: HashMap<String, SessionTreeEntry>,
    /// label 缓存。
    labels_by_id: HashMap<String, String>,
    /// 当前 leaf。
    leaf_id: Option<String>,
}

#[async_trait]
impl SessionStorage for InMemorySessionStorage {
    /// 打开内存会话存储。
    async fn open(_path: &Path, session_id: &str) -> HarnessResult<Arc<Self>> {
        let metadata = SessionMetadata {
            id: session_id.to_string(),
            name: String::new(),
            created_at: now_iso(),
            cwd: String::new(),
            path: String::new(),
            parent_session_path: None,
        };
        Self::from_loaded(PathBuf::new(), metadata, Vec::new(), None)
    }

    /// 创建内存会话存储。
    async fn create(path: &Path, options: SessionCreateStorageOptions) -> HarnessResult<Arc<Self>> {
        let metadata = SessionMetadata {
            id: options.session_id,
            name: options.name,
            created_at: now_iso(),
            cwd: options.cwd,
            path: path.display().to_string(),
            parent_session_path: options.parent_session_path,
        };
        Self::from_loaded(path.to_path_buf(), metadata, Vec::new(), None)
    }

    /// 基于已加载数据构造存储实例。
    fn from_loaded(
        _path: PathBuf,
        metadata: SessionMetadata,
        entries: Vec<SessionTreeEntry>,
        leaf_id: Option<String>,
    ) -> HarnessResult<Arc<Self>> {
        let by_id = entries.iter().cloned().map(|entry| (entry.id().to_string(), entry)).collect::<HashMap<_, _>>();
        let labels_by_id = build_labels_by_id(&entries);
        let leaf_id = leaf_id.or_else(|| entries.last().map(|entry| entry.id().to_string()));
        if let Some(leaf_id) = &leaf_id {
            if !by_id.contains_key(leaf_id) {
                return Err(HarnessError::Message(format!("Entry {leaf_id} not found")));
            }
        }
        Ok(Arc::new(Self {
            inner: RwLock::new(InMemorySessionStorageInner { entries, by_id, labels_by_id, leaf_id }),
            metadata: RwLock::new(metadata),
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

    /// 重命名内存会话。
    async fn rename(&self, name: String) -> HarnessResult<()> {
        let mut metadata = self.metadata.write().await;
        metadata.name = name;
        Ok(())
    }

    /// 返回当前 leaf。
    async fn get_leaf_id(&self) -> Option<String> {
        self.inner.read().await.leaf_id.clone()
    }

    /// 设置当前 leaf。
    async fn set_leaf_id(&self, leaf_id: Option<String>) -> HarnessResult<()> {
        let mut inner = self.inner.write().await;
        if let Some(leaf_id) = &leaf_id {
            if !inner.by_id.contains_key(leaf_id) {
                return Err(HarnessError::Message(format!("Entry {leaf_id} not found")));
            }
        }
        inner.leaf_id = leaf_id;
        Ok(())
    }

    /// 创建新条目 id。
    async fn create_entry_id(&self) -> String {
        let inner = self.inner.read().await;
        generate_entry_id(&inner.by_id)
    }

    /// 追加条目。
    async fn append_entry(&self, entry: SessionTreeEntry) -> HarnessResult<()> {
        let mut inner = self.inner.write().await;
        update_label_cache(&mut inner.labels_by_id, &entry);
        inner.leaf_id = Some(entry.id().to_string());
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

/// 更新 label 缓存。
pub fn update_label_cache(labels_by_id: &mut HashMap<String, String>, entry: &SessionTreeEntry) {
    if let SessionTreeEntry::Label { target_id, label, .. } = entry {
        let label = label.as_deref().unwrap_or_default().trim();
        if label.is_empty() {
            labels_by_id.remove(target_id);
        } else {
            labels_by_id.insert(target_id.clone(), label.to_string());
        }
    }
}

/// 基于完整条目数组重建 label 缓存。
pub fn build_labels_by_id(entries: &[SessionTreeEntry]) -> HashMap<String, String> {
    let mut labels_by_id = HashMap::new();
    for entry in entries {
        update_label_cache(&mut labels_by_id, entry);
    }
    labels_by_id
}

/// 生成不与现有条目冲突的 id。
pub fn generate_entry_id(by_id: &HashMap<String, SessionTreeEntry>) -> String {
    for _ in 0..100 {
        let id = Uuid::new_v4().to_string().chars().take(8).collect::<String>();
        if !by_id.contains_key(&id) {
            return id;
        }
    }
    Uuid::new_v4().to_string()
}
