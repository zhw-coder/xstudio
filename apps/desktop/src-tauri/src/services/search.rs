use std::collections::HashMap;

use serde_json::Value;
use tool::ToolRegistry;

use crate::{
    dto::{SaveSearchEngineInput, SearchEngineOutput},
    error::{AppError, AppResult},
    infra::db,
    models::SearchEngine,
};

/// 返回按领域分组的工具库搜索实体名称。
pub fn list_engines() -> Vec<Vec<String>> {
    tool::SearchRegistry::global().engines()
}

/// 查询搜索实体配置；优先读取数据库，未保存时返回工具库默认参数。
/// @param engine 搜索实体名称。
pub fn get_engine(engine: &str) -> AppResult<SearchEngineOutput> {
    Ok(SearchEngineOutput {
        engine: engine.to_string(),
        enabled: false,
        parameters: default_parameters(engine)?,
    })
}

/// 查询数据库全部已保存搜索实体配置。
/// @param app Tauri 应用句柄。
pub async fn list_configs(app: &tauri::AppHandle) -> AppResult<Vec<SearchEngineOutput>> {
    SearchEngine::list(db::pool(app).await?)
        .await?
        .into_iter()
        .map(|record| {
            Ok(SearchEngineOutput {
                engine: record.engine,
                enabled: record.enabled,
                parameters: serde_json::from_str(&record.parameters)?,
            })
        })
        .collect()
}

/// 保存搜索实体配置。
/// @param app Tauri 应用句柄。
/// @param input 搜索实体配置请求。
pub async fn save_config(
    app: &tauri::AppHandle,
    input: SaveSearchEngineInput,
) -> AppResult<SearchEngineOutput> {
    default_parameters(&input.engine)?;
    let record = SearchEngine::save(
        db::pool(app).await?,
        SearchEngine {
            engine: input.engine,
            enabled: input.enabled,
            parameters: serde_json::to_string(&input.parameters)?,
        },
    )
    .await?;
    init(app).await?;
    Ok(SearchEngineOutput {
        engine: record.engine,
        enabled: record.enabled,
        parameters: serde_json::from_str(&record.parameters)?,
    })
}

/// 加载并初始化搜索工具配置。
/// @param app Tauri 应用句柄。
pub async fn init(app: &tauri::AppHandle) -> AppResult<()> {
    ToolRegistry::global()
        .init(HashMap::from([(
            "search".to_string(),
            serde_json::to_value(enabled_configs(app).await?)?,
        )]))
        .map_err(|error| AppError::AiHarness(error.to_string()))
}

/// 加载全部已启用搜索实体的运行时配置。
/// @param app Tauri 应用句柄。
pub async fn enabled_configs(app: &tauri::AppHandle) -> AppResult<HashMap<String, Value>> {
    let records = SearchEngine::list_enabled(db::pool(app).await?).await?;
    records
        .into_iter()
        .map(|record| {
            let parameters = serde_json::from_str(&record.parameters)?;
            Ok((record.engine, parameters))
        })
        .collect()
}

/// 返回搜索实体默认参数，未知实体返回业务错误。
/// @param engine 搜索实体名称。
fn default_parameters(engine: &str) -> AppResult<Value> {
    tool::SearchRegistry::global()
        .get(engine)
        .ok_or_else(|| AppError::SearchEngineNotFound(engine.to_string()))?
        .parameters()
        .map_err(|error| AppError::AiHarness(error.to_string()))
}
