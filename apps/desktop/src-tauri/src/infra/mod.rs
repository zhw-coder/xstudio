pub mod db;

use std::path::PathBuf;

use crate::error::AppResult;

/// 获取应用目录并确保目录存在。
pub fn app_dir() -> AppResult<PathBuf> {
    let app_dir = std::env::home_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "用户家目录不存在"))?
        .join(".xstudio");
    std::fs::create_dir_all(&app_dir)?;
    Ok(app_dir)
}
