//! 会话仓库共用的 id、时间戳、fork 条目裁剪逻辑。

use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

use crate::agent::harness::{session::session::Session, types::*};

/// 创建全局唯一会话 id。
pub fn create_session_id() -> String {
    Uuid::now_v7().to_string()
}

/// 创建 RFC3339 时间戳。
pub fn create_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| OffsetDateTime::now_utc().unix_timestamp().to_string())
}

/// 计算 fork 时应复制的条目列表。
pub async fn get_entries_to_fork(session: &Session, options: &SessionForkOptions) -> Vec<SessionTreeEntry> {
    let mut branch = session.get_branch(options.entry_id.as_deref()).await;
    match options.position.as_ref().unwrap_or(&SessionForkPosition::At) {
        SessionForkPosition::At => branch,
        SessionForkPosition::Before => {
            if options.entry_id.is_some() {
                branch.pop();
            }
            branch
        }
    }
}
