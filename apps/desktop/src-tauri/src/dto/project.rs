use serde::{Deserialize, Serialize};

/// 保存项目请求。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProjectInput {
    /// 项目路径。
    pub path: String,
}

/// 删除项目请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectInput {
    /// 项目路径。
    pub path: String,
}

/// 项目返回数据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOutput {
    /// 项目路径。
    pub path: String,
    /// 最近更新时间的 Unix 毫秒时间戳。
    pub updated_at: i64,
}
