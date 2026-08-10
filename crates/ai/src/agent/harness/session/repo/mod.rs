use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use crate::agent::harness::{types::SessionRepo, HarnessError, HarnessResult};

pub mod jsonl;
pub mod memory;
pub mod shared;
pub mod sqlite;

pub use jsonl::*;
pub use memory::*;
pub use shared::*;
pub use sqlite::*;

/// 全局静态会话仓储注册表。
static GLOBAL_SESSION_REPO_REGISTRY: OnceLock<Arc<SessionRepoRegistry>> = OnceLock::new();

/// 会话仓储注册表。
#[derive(Clone)]
pub struct SessionRepoRegistry {
    /// 仓储名称到共享仓储实例的映射。
    repos: HashMap<String, Arc<dyn SessionRepo>>,
}

impl SessionRepoRegistry {
    /// 创建并注册全部内置持久化会话仓储。
    pub fn new() -> Self {
        let mut registry = Self { repos: HashMap::new() };
        registry.register(InMemorySessionRepo::name(), Arc::new(InMemorySessionRepo::new()));
        registry.register(JsonlSessionRepo::name(), Arc::new(JsonlSessionRepo::new()));
        registry.register(SqliteSessionRepo::name(), Arc::new(SqliteSessionRepo::new()));
        registry
    }

    /// 返回全局静态单例注册表。
    pub fn global() -> &'static Arc<Self> {
        GLOBAL_SESSION_REPO_REGISTRY.get_or_init(|| Arc::new(Self::new()))
    }

    /// 注册会话仓储。
    /// @param name 仓储名称。
    /// @param repo 会话仓储实例。
    pub fn register(&mut self, name: impl Into<String>, repo: Arc<dyn SessionRepo>) {
        self.repos.insert(name.into(), repo);
    }

    /// 返回全部已注册仓储名称。
    pub fn names(&self) -> Vec<String> {
        let mut repos = self.repos.keys().cloned().collect::<Vec<_>>();
        repos.sort_unstable();
        repos
    }

    /// 初始化指定会话仓储实例。
    /// @param name 仓储名称。
    /// @param path 会话仓储根路径。
    pub async fn init(&self, name: &str, path: impl Into<PathBuf>) -> HarnessResult<()> {
        self.repos
            .get(name)
            .cloned()
            .ok_or_else(|| HarnessError::Message(format!("Unknown session repository: {name}")))?
            .init(path.into())
            .await
    }

    /// 按名称获取已注册会话仓储实例。
    /// @param name 仓储名称。
    pub fn get(&self, name: &str) -> HarnessResult<Arc<dyn SessionRepo>> {
        self.repos
            .get(name)
            .cloned()
            .ok_or_else(|| HarnessError::Message(format!("Unknown session repository: {name}")))
    }
}
