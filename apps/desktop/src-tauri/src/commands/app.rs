//! 桌面应用通用命令。

/// 使用系统默认浏览器打开外部链接。
/// @param url 需要打开的完整外部链接。
#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || open::that(url))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}
