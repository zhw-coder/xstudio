use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 搜索实体配置返回数据。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEngineOutput {
    /// 搜索实体名称。
    pub engine: String,
    /// 是否已启用。
    pub enabled: bool,
    /// 搜索实体参数。
    pub parameters: Value,
}

/// 保存搜索实体配置请求。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSearchEngineInput {
    /// 搜索实体名称。
    pub engine: String,
    /// 是否在构建 SearchTool 时使用。
    pub enabled: bool,
    /// 搜索实体参数。
    pub parameters: Value,
}
