use ormlite::Model as OrmliteModel;
use serde::{Deserialize, Serialize};

use crate::{
    error::AppResult,
    infra::db::{DbPool, Migratable},
};

/// 项目最大保存数量。
const MAX_PROJECTS: i64 = 15;

/// 项目的持久化记录。
#[derive(Clone, Debug, Serialize, Deserialize, OrmliteModel)]
#[serde(rename_all = "camelCase")]
#[ormlite(table = "projects")]
pub struct Project {
    /// 项目绝对或相对路径，同时作为唯一标识。
    #[ormlite(primary_key)]
    pub path: String,
    /// 最近更新时间的 Unix 毫秒时间戳。
    pub updated_at: i64,
}

impl Project {
    /// 按更新时间倒序查询全部项目。
    /// @param pool SQLite 连接池。
    pub async fn list(pool: &DbPool) -> AppResult<Vec<Self>> {
        Self::select()
            .order_desc("updated_at")
            .order_asc("path")
            .fetch_all(pool)
            .await
            .map_err(Into::into)
    }

    /// 保存项目路径，并更新其最近更新时间。
    /// @param pool SQLite 连接池。
    /// @param path 项目路径。
    /// @param updated_at 最近更新时间的 Unix 毫秒时间戳。
    pub async fn save(pool: &DbPool, path: String, updated_at: i64) -> AppResult<Self> {
        if let Some(mut project) = Self::select()
            .where_bind("path = ?", &path)
            .fetch_optional(pool)
            .await?
        {
            project.updated_at = updated_at;
            return project.update_all_fields(pool).await.map_err(Into::into);
        }

        Self::delete_oldest_when_full(pool).await?;
        Self { path, updated_at }
            .insert(pool)
            .await
            .map_err(Into::into)
    }

    /// 按路径删除项目记录。
    /// @param pool SQLite 连接池。
    /// @param path 项目路径。
    pub async fn delete_by_path(pool: &DbPool, path: &str) -> AppResult<()> {
        ormlite::query("DELETE FROM projects WHERE path = ?")
            .bind(path)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// 项目数量达到上限时删除更新时间最早的一条记录。
    /// @param pool SQLite 连接池。
    async fn delete_oldest_when_full(pool: &DbPool) -> AppResult<()> {
        if (Self::list(pool).await?.len() as i64) < MAX_PROJECTS {
            return Ok(());
        }

        ormlite::query(
            "DELETE FROM projects
             WHERE path = (
                 SELECT path FROM projects
                 ORDER BY updated_at ASC, path ASC
                 LIMIT 1
             )",
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}

impl Migratable for Project {
    /// 执行项目表迁移。
    /// @param pool SQLite 连接池。
    async fn migrate(pool: &DbPool) -> AppResult<()> {
        ormlite::query(
            "CREATE TABLE IF NOT EXISTS projects (
                path TEXT PRIMARY KEY NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(pool)
        .await?;

        ormlite::query(
            "INSERT OR IGNORE INTO projects (path, updated_at)
                             VALUES ('./', 0)",
        )
        .execute(pool)
        .await?;

        ormlite::query(
            "CREATE INDEX IF NOT EXISTS idx_projects_updated_at
             ON projects(updated_at DESC)",
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
