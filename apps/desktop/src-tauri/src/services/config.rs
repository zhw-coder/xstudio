use crate::{
    error::{AppError, AppResult},
    infra::{self, db},
    models::Config,
};
use ai::agent::harness::session::repo::SessionRepoRegistry;

/// 获取默认配置。
/// @param app Tauri 应用句柄。
pub async fn get_config(app: &tauri::AppHandle) -> AppResult<Config> {
    Config::get_or_create(db::pool(app).await?).await
}

/// 保存默认配置。
/// @param app Tauri 应用句柄。
/// @param config 前端传入的配置数据。
pub async fn set_config(app: &tauri::AppHandle, config: Config) -> AppResult<Config> {
    if !(1..=100).contains(&config.compact_ratio) {
        return Err(AppError::InvalidCompactRatio(config.compact_ratio));
    }

    let config = Config::set_config(db::pool(app).await?, config).await?;
    init_session_repos(&config.path).await?;
    Ok(config)
}

/// 在系统文件管理器中打开应用数据目录。
pub async fn open_app_dir() -> AppResult<()> {
    let app_dir = infra::app_dir()?;
    tokio::task::spawn_blocking(move || open::that(app_dir))
        .await
        .map_err(|error| AppError::OpenPath(error.to_string()))??;
    Ok(())
}

/// 加载并初始化当前会话仓储配置。
/// @param app Tauri 应用句柄。
pub async fn init(app: &tauri::AppHandle) -> AppResult<()> {
    let config = get_config(app).await?;
    init_session_repos(&config.path).await?;
    Ok(())
}

/// 使用指定根路径初始化全部已注册会话仓储。
/// @param path 会话仓储根路径。
async fn init_session_repos(path: &str) -> AppResult<()> {
    let registry = SessionRepoRegistry::global();
    for name in registry.names() {
        registry.init(&name, path).await?;
    }
    Ok(())
}
