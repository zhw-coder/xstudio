//! SQLite 会话仓库实现。
//! 使用仓储目录内的单个 SQLite 数据库文件按 cwd / session id 管理 harness 会话。

use async_trait::async_trait;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::RwLock;

use crate::agent::harness::{
    session::{
        repo::shared::{create_session_id, get_entries_to_fork},
        storage::sqlite::{
            delete_sqlite_session, list_sqlite_session_metadata, sqlite_session_exists, SqliteSessionStorage,
        },
        Session, SessionHandle,
    },
    types::*,
    HarnessResult,
};

/// SQLite 会话数据库文件名。
const DATABASE_FILE_NAME: &str = "db.sqlite";

/// SQLite 会话仓库。
#[derive(Clone, Debug)]
pub struct SqliteSessionRepo {
    /// SQLite 数据库文件路径。
    db_path: Arc<RwLock<PathBuf>>,
}

#[async_trait]
impl SessionRepo for SqliteSessionRepo {
    /// 返回供界面展示的仓储名称。
    fn name() -> &'static str {
        "Sqlite"
    }

    /// 创建使用默认数据库路径的 SQLite 会话仓库。
    fn new() -> Self {
        Self { db_path: Arc::new(RwLock::new(PathBuf::new())) }
    }

    /// 设置 SQLite 会话仓库目录。
    /// @param path 存放 db.sqlite 的仓储目录。
    async fn init(&self, path: PathBuf) -> HarnessResult<()> {
        *self.db_path.write().await = path.join(DATABASE_FILE_NAME);
        Ok(())
    }

    /// 创建会话。
    async fn create(&self, options: CreateOptions) -> HarnessResult<SessionHandle> {
        let session_id = options.id.unwrap_or_else(create_session_id);
        let db_path = self.db_path.read().await;
        let storage = SqliteSessionStorage::create(
            &*db_path,
            SessionCreateStorageOptions {
                cwd: options.cwd,
                session_id,
                name: String::new(),
                parent_session_path: options.parent_session_path,
            },
        )
        .await?;
        Ok(Arc::new(Session::new(storage)))
    }

    /// 打开会话。
    async fn open(&self, metadata: SessionMetadata) -> HarnessResult<SessionHandle> {
        let db_path = self.db_path.read().await;
        let storage = SqliteSessionStorage::open(&*db_path, &metadata.id).await?;
        Ok(Arc::new(Session::new(storage)))
    }

    /// fork 一个 SQLite 会话。
    async fn fork(&self, from_session: &Session, options: SessionForkOptions) -> HarnessResult<SessionHandle> {
        let metadata = from_session.with_metadata_guard().await;
        let (cwd, parent_session_path, source_name) =
            (metadata.cwd.to_string(), metadata.path.to_string(), metadata.name.to_string());
        let entries = get_entries_to_fork(from_session, &options).await;
        let id = options.id.unwrap_or_else(create_session_id);
        let db_path = self.db_path.read().await;
        let storage = SqliteSessionStorage::create(
            &*db_path,
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

    /// 判断会话是否存在。
    async fn exists(&self, metadata: &SessionMetadata) -> bool {
        let db_path = self.db_path.read().await;
        sqlite_session_exists(&*db_path, metadata.id.clone()).await
    }

    /// 重命名 SQLite 会话。
    async fn rename(&self, metadata: SessionMetadata, name: String) -> HarnessResult<()> {
        let session = self.open(metadata).await?;
        session.get_storage().rename(name).await
    }

    /// 列举会话。
    async fn list(&self, options: ListOptions) -> HarnessResult<Vec<SessionMetadata>> {
        let db_path = self.db_path.read().await;
        list_sqlite_session_metadata(&*db_path, options.cwd).await
    }

    /// 删除会话。
    async fn delete(&self, metadata: SessionMetadata) -> HarnessResult<()> {
        let db_path = self.db_path.read().await;
        delete_sqlite_session(&*db_path, metadata.id).await
    }
}
