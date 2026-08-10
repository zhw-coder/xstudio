use std::{collections::HashMap, future::Future, sync::Arc};

use tokio::sync::Mutex;

/// 同一路径的写入锁表；不同文件可并发修改。
#[derive(Clone, Debug, Default)]
pub struct FileMutationQueue {
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl FileMutationQueue {
    /// 创建空的文件修改队列。
    pub fn new() -> Self {
        Self::default()
    }

    /// 在目标文件的独占锁下执行异步修改。
    pub async fn with_file_mutation<T, F, Fut>(&self, path: &str, operation: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let key = path.to_string();
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        operation().await
    }
}
