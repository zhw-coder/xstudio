use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use ai::agent::{env::ExecutionEnv, harness::AgentHarness};
use tokio::sync::{oneshot, Mutex as AsyncMutex, OnceCell, RwLock};

use crate::{
    error::{AppError, AppResult},
    services,
};

/// 桌面端后端内存级长久全局数据。
pub struct Context {
    /// 会话 id 到 AgentHarness 的映射。
    pub agent_harnesses: Mutex<HashMap<String, Arc<AgentHarness>>>,
    /// 等待客户端结算的工具审批请求。
    pub tool_approvals: AsyncMutex<HashMap<String, ToolApprovalWaiter>>,
}

/// 等待客户端决策的工具审批请求。
pub struct ToolApprovalWaiter {
    /// 所属会话 id。
    pub session_id: String,
    /// 审批结算发送端。
    pub sender: oneshot::Sender<bool>,
}

/// 桌面端后端异步全局数据。
pub struct ContextAsync {
    /// 当前项目共享的本地执行环境。
    pub env: RwLock<Arc<dyn ExecutionEnv>>,
}

/// 同步全局 Context 单例。
static CONTEXT: OnceLock<Context> = OnceLock::new();

/// 异步全局 Context 单例。
static CONTEXT_ASYNC: OnceCell<ContextAsync> = OnceCell::const_new();

/// 获取同步全局 Context。
pub fn context() -> &'static Context {
    CONTEXT.get_or_init(|| Context {
        agent_harnesses: Mutex::new(HashMap::new()),
        tool_approvals: AsyncMutex::new(HashMap::new()),
    })
}

/// 懒加载获取异步全局 Context。
/// @param app Tauri 应用句柄。
pub async fn context_async(app: &tauri::AppHandle) -> AppResult<&'static ContextAsync> {
    CONTEXT_ASYNC
        .get_or_try_init(|| async {
            let cwd = services::project::latest_project_path(app)
                .await?
                .ok_or(AppError::CurrentProjectNotFound)?;
            let env = plugins::PluginRuntime::global()
                .and_then(|runtime| runtime.create_env(std::path::Path::new(&cwd)))
                .map_err(|error| AppError::AiHarness(error.to_string()))?;
            Ok(ContextAsync {
                env: RwLock::new(env),
            })
        })
        .await
}

/// 按新工作目录重置执行环境与全部会话 Harness。
/// @param cwd 新项目工作目录。
pub async fn reset_async(cwd: &str) -> AppResult<()> {
    let env = plugins::PluginRuntime::global()
        .and_then(|runtime| runtime.create_env(std::path::Path::new(cwd)))
        .map_err(|error| AppError::AiHarness(error.to_string()))?;
    let context_async = CONTEXT_ASYNC
        .get_or_init(|| async {
            ContextAsync {
                env: RwLock::new(Arc::clone(&env)),
            }
        })
        .await;
    let agent_harnesses = context()
        .agent_harnesses
        .lock()
        .map_err(|error| AppError::ContextLock(error.to_string()))?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for agent_harness in agent_harnesses {
        if !agent_harness.is_idle().await {
            return Err(AppError::AiHarness(
                "重置项目要求全部会话处于空闲状态".to_string(),
            ));
        }
    }
    *context_async.env.write().await = env;
    context()
        .agent_harnesses
        .lock()
        .map_err(|error| AppError::ContextLock(error.to_string()))?
        .clear();
    Ok(())
}
