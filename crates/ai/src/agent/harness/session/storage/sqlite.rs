//! SQLite 文件版 `SessionStorage` 实现。
//! 使用单个 SQLite 数据库文件保存多个会话，按 `session_id` 追加会话树条目。

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    sync::{RwLock, RwLockReadGuard},
    task,
};

use crate::agent::harness::{
    session::{
        session::now_iso,
        storage::memory::{build_labels_by_id, generate_entry_id, update_label_cache},
    },
    types::*,
    HarnessError, HarnessResult,
};

/// SQLite session schema 版本。
const SQLITE_SCHEMA_VERSION: i32 = 1;

/// SQLite 文件版会话存储后端。
#[derive(Debug)]
pub struct SqliteSessionStorage {
    /// SQLite 数据库绝对路径。
    db_path: PathBuf,
    /// 元信息。
    metadata: RwLock<SessionMetadata>,
    /// 内部可变状态。
    inner: RwLock<SqliteSessionStorageInner>,
}

/// SQLite 存储可变状态。
#[derive(Clone, Debug, Default)]
struct SqliteSessionStorageInner {
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
impl SessionStorage for SqliteSessionStorage {
    /// 从已存在的 SQLite 数据库加载指定会话。
    async fn open(path: &Path, session_id: &str) -> HarnessResult<Arc<Self>> {
        let resolved = absolute_path(path)?;
        let (metadata, entries, leaf_id) = load_sqlite_storage(resolved.clone(), session_id.to_string()).await?;
        Self::from_loaded(resolved, metadata, entries, leaf_id)
    }

    /// 创建一份新的 SQLite 会话。
    async fn create(path: &Path, options: SessionCreateStorageOptions) -> HarnessResult<Arc<Self>> {
        let resolved = absolute_path(path)?;
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let metadata = SessionMetadata {
            id: options.session_id,
            name: options.name,
            created_at: now_iso(),
            cwd: options.cwd,
            path: resolved.display().to_string(),
            parent_session_path: options.parent_session_path,
        };
        create_sqlite_session(resolved.clone(), metadata.clone()).await?;
        Self::from_loaded(resolved, metadata, Vec::new(), None)
    }

    /// 基于已加载数据构造存储实例。
    fn from_loaded(
        db_path: PathBuf,
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
            db_path,
            metadata: RwLock::new(metadata),
            inner: RwLock::new(SqliteSessionStorageInner { entries, by_id, labels_by_id, current_leaf_id: leaf_id }),
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

    /// 重命名 SQLite 会话。
    async fn rename(&self, name: String) -> HarnessResult<()> {
        let mut metadata = self.metadata.write().await;
        rename_sqlite_session(&self.db_path, &metadata.id, &name).await?;
        metadata.name = name;
        Ok(())
    }

    /// 返回当前 leaf。
    async fn get_leaf_id(&self) -> Option<String> {
        self.inner.read().await.current_leaf_id.clone()
    }

    /// 设置当前 leaf。
    async fn set_leaf_id(&self, leaf_id: Option<String>) -> HarnessResult<()> {
        {
            let inner = self.inner.read().await;
            if let Some(leaf_id) = &leaf_id {
                if !inner.by_id.contains_key(leaf_id) {
                    return Err(HarnessError::Message(format!("Entry {leaf_id} not found")));
                }
            }
        }
        let metadata = self.metadata.read().await;
        set_sqlite_leaf_id(&self.db_path, &metadata.id, leaf_id.as_deref()).await?;
        drop(metadata);
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
        let db_path = self.db_path.clone();
        let session_id = self.metadata.read().await.id.clone();
        let ordinal = self.inner.read().await.entries.len() as i64;
        let entry_for_db = entry.clone();
        task::spawn_blocking(move || append_sqlite_entry(&db_path, &session_id, &entry_for_db, ordinal))
            .await
            .map_err(|error| HarnessError::Message(format!("SQLite append task failed: {error}")))??;

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

/// 读取 SQLite 数据库中的会话元信息列表。
pub async fn list_sqlite_session_metadata(
    db_path: impl AsRef<Path>,
    cwd: Option<String>,
) -> HarnessResult<Vec<SessionMetadata>> {
    let resolved = absolute_path(db_path.as_ref())?;
    task::spawn_blocking(move || list_sqlite_session_metadata_sync(&resolved, cwd))
        .await
        .map_err(|error| HarnessError::Message(format!("SQLite list task failed: {error}")))?
}

/// 删除 SQLite 数据库中的会话。
pub async fn delete_sqlite_session(db_path: impl AsRef<Path>, session_id: String) -> HarnessResult<()> {
    let resolved = absolute_path(db_path.as_ref())?;
    task::spawn_blocking(move || delete_sqlite_session_sync(&resolved, &session_id))
        .await
        .map_err(|error| HarnessError::Message(format!("SQLite delete task failed: {error}")))?
}

/// 重命名 SQLite 数据库中的会话。
pub async fn rename_sqlite_session(db_path: impl AsRef<Path>, session_id: &str, name: &str) -> HarnessResult<()> {
    let resolved = absolute_path(db_path.as_ref())?;
    let session_id = session_id.to_string();
    let name = name.to_string();
    task::spawn_blocking(move || rename_sqlite_session_sync(&resolved, &session_id, &name))
        .await
        .map_err(|error| HarnessError::Message(format!("SQLite rename task failed: {error}")))?
}

/// 判断 SQLite 数据库中的会话是否存在。
pub async fn sqlite_session_exists(db_path: impl AsRef<Path>, session_id: String) -> bool {
    let Ok(resolved) = absolute_path(db_path.as_ref()) else { return false };
    task::spawn_blocking(move || sqlite_session_exists_sync(&resolved, &session_id).unwrap_or(false))
        .await
        .unwrap_or(false)
}

/// 创建 SQLite 会话。
async fn create_sqlite_session(db_path: PathBuf, metadata: SessionMetadata) -> HarnessResult<()> {
    task::spawn_blocking(move || create_sqlite_session_sync(&db_path, &metadata))
        .await
        .map_err(|error| HarnessError::Message(format!("SQLite create task failed: {error}")))?
}

/// 加载 SQLite 会话。
async fn load_sqlite_storage(
    db_path: PathBuf,
    session_id: String,
) -> HarnessResult<(SessionMetadata, Vec<SessionTreeEntry>, Option<String>)> {
    task::spawn_blocking(move || load_sqlite_storage_sync(&db_path, &session_id))
        .await
        .map_err(|error| HarnessError::Message(format!("SQLite open task failed: {error}")))?
}

/// 设置 SQLite leaf。
async fn set_sqlite_leaf_id(db_path: impl AsRef<Path>, session_id: &str, leaf_id: Option<&str>) -> HarnessResult<()> {
    let resolved = absolute_path(db_path.as_ref())?;
    let session_id = session_id.to_string();
    let leaf_id = leaf_id.map(str::to_string);
    task::spawn_blocking(move || set_sqlite_leaf_id_sync(&resolved, &session_id, leaf_id.as_deref()))
        .await
        .map_err(|error| HarnessError::Message(format!("SQLite set leaf task failed: {error}")))?
}

/// 创建 SQLite 会话。
fn create_sqlite_session_sync(db_path: &Path, metadata: &SessionMetadata) -> HarnessResult<()> {
    let conn = open_connection(db_path)?;
    conn.execute(
        "INSERT INTO sessions (id, name, created_at, cwd, path, parent_session_path, current_leaf_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
        params![metadata.id, metadata.name, metadata.created_at, metadata.cwd, metadata.path, metadata.parent_session_path],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

/// 加载 SQLite 会话。
fn load_sqlite_storage_sync(
    db_path: &Path,
    session_id: &str,
) -> HarnessResult<(SessionMetadata, Vec<SessionTreeEntry>, Option<String>)> {
    let conn = open_connection(db_path)?;
    let metadata_and_leaf = conn
        .query_row(
            "SELECT id, name, created_at, cwd, path, parent_session_path, current_leaf_id FROM sessions WHERE id = ?1",
            params![session_id],
            |row| {
                Ok((
                    SessionMetadata {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                        cwd: row.get(3)?,
                        path: row.get(4)?,
                        parent_session_path: row.get(5)?,
                    },
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite_error)?;
    let Some((metadata, leaf_id)) = metadata_and_leaf else {
        return Err(HarnessError::Message(format!("SQLite session {session_id} not found")));
    };
    let mut stmt = conn
        .prepare("SELECT payload_json FROM entries WHERE session_id = ?1 ORDER BY ordinal ASC")
        .map_err(sqlite_error)?;
    let rows = stmt.query_map(params![session_id], |row| row.get::<_, String>(0)).map_err(sqlite_error)?;
    let mut entries = Vec::new();
    for row in rows {
        let payload = row.map_err(sqlite_error)?;
        entries.push(serde_json::from_str::<SessionTreeEntry>(&payload)?);
    }
    Ok((metadata, entries, leaf_id))
}

/// 追加 SQLite 条目。
fn append_sqlite_entry(db_path: &Path, session_id: &str, entry: &SessionTreeEntry, ordinal: i64) -> HarnessResult<()> {
    let mut conn = open_connection(db_path)?;
    let tx = conn.transaction().map_err(sqlite_error)?;
    tx.execute(
        "INSERT INTO entries (session_id, id, parent_id, timestamp, entry_type, payload_json, ordinal) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session_id,
            entry.id(),
            entry.parent_id(),
            entry.base().timestamp,
            entry.entry_type(),
            serde_json::to_string(entry)?,
            ordinal,
        ],
    )
    .map_err(sqlite_error)?;
    tx.execute("UPDATE sessions SET current_leaf_id = ?1 WHERE id = ?2", params![entry.id(), session_id])
        .map_err(sqlite_error)?;
    tx.commit().map_err(sqlite_error)?;
    Ok(())
}

/// 设置 SQLite leaf。
fn set_sqlite_leaf_id_sync(db_path: &Path, session_id: &str, leaf_id: Option<&str>) -> HarnessResult<()> {
    let conn = open_connection(db_path)?;
    conn.execute("UPDATE sessions SET current_leaf_id = ?1 WHERE id = ?2", params![leaf_id, session_id])
        .map_err(sqlite_error)?;
    Ok(())
}

/// 列举 SQLite 会话元信息。
fn list_sqlite_session_metadata_sync(db_path: &Path, cwd: Option<String>) -> HarnessResult<Vec<SessionMetadata>> {
    let conn = open_connection(db_path)?;
    let mut results = Vec::new();
    if let Some(cwd) = cwd {
        let mut stmt = conn.prepare(
            "SELECT id, name, created_at, cwd, path, parent_session_path FROM sessions WHERE cwd = ?1 ORDER BY created_at DESC",
        )
        .map_err(sqlite_error)?;
        let rows = stmt.query_map(params![cwd], row_to_metadata).map_err(sqlite_error)?;
        for row in rows {
            results.push(row.map_err(sqlite_error)?);
        }
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, created_at, cwd, path, parent_session_path FROM sessions ORDER BY created_at DESC",
            )
            .map_err(sqlite_error)?;
        let rows = stmt.query_map([], row_to_metadata).map_err(sqlite_error)?;
        for row in rows {
            results.push(row.map_err(sqlite_error)?);
        }
    }
    Ok(results)
}

/// 重命名 SQLite 会话。
fn rename_sqlite_session_sync(db_path: &Path, session_id: &str, name: &str) -> HarnessResult<()> {
    let conn = open_connection(db_path)?;
    let changed =
        conn.execute("UPDATE sessions SET name = ?1 WHERE id = ?2", params![name, session_id]).map_err(sqlite_error)?;
    if changed == 0 {
        return Err(HarnessError::Message(format!("SQLite session {session_id} not found")));
    }
    Ok(())
}

/// 删除 SQLite 会话。
fn delete_sqlite_session_sync(db_path: &Path, session_id: &str) -> HarnessResult<()> {
    let mut conn = open_connection(db_path)?;
    let tx = conn.transaction().map_err(sqlite_error)?;
    tx.execute("DELETE FROM entries WHERE session_id = ?1", params![session_id]).map_err(sqlite_error)?;
    tx.execute("DELETE FROM sessions WHERE id = ?1", params![session_id]).map_err(sqlite_error)?;
    tx.commit().map_err(sqlite_error)?;
    Ok(())
}

/// 判断 SQLite 会话是否存在。
fn sqlite_session_exists_sync(db_path: &Path, session_id: &str) -> HarnessResult<bool> {
    let conn = open_connection(db_path)?;
    let exists = conn
        .query_row("SELECT 1 FROM sessions WHERE id = ?1", params![session_id], |_| Ok(()))
        .optional()
        .map_err(sqlite_error)?
        .is_some();
    Ok(exists)
}

/// 打开连接并初始化 schema。
fn open_connection(db_path: &Path) -> HarnessResult<Connection> {
    let conn = Connection::open(db_path).map_err(sqlite_error)?;
    conn.pragma_update(None, "journal_mode", "WAL").map_err(sqlite_error)?;
    conn.pragma_update(None, "foreign_keys", "ON").map_err(sqlite_error)?;
    migrate_schema(&conn)?;
    Ok(conn)
}

/// 初始化 SQLite schema。
fn migrate_schema(conn: &Connection) -> HarnessResult<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            cwd TEXT NOT NULL,
            path TEXT NOT NULL,
            parent_session_path TEXT NULL,
            current_leaf_id TEXT NULL
        );
        CREATE TABLE IF NOT EXISTS entries (
            session_id TEXT NOT NULL,
            id TEXT NOT NULL,
            parent_id TEXT NULL,
            timestamp TEXT NOT NULL,
            entry_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            PRIMARY KEY (session_id, id),
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_entries_session_ordinal ON entries(session_id, ordinal);
        CREATE INDEX IF NOT EXISTS idx_entries_session_type ON entries(session_id, entry_type);
        CREATE INDEX IF NOT EXISTS idx_sessions_cwd ON sessions(cwd);
        CREATE INDEX IF NOT EXISTS idx_sessions_created_at ON sessions(created_at);
        ",
    )
    .map_err(sqlite_error)?;
    conn.pragma_update(None, "user_version", SQLITE_SCHEMA_VERSION).map_err(sqlite_error)?;
    Ok(())
}

/// 把查询行转换为 SQLite 元信息。
fn row_to_metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionMetadata> {
    Ok(SessionMetadata {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        cwd: row.get(3)?,
        path: row.get(4)?,
        parent_session_path: row.get(5)?,
    })
}

/// 获取绝对路径。
fn absolute_path(path: &Path) -> std::io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// 转换 SQLite 错误。
fn sqlite_error(error: rusqlite::Error) -> HarnessError {
    HarnessError::Message(error.to_string())
}
