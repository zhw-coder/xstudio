//! Git 检查点集成测试。

use ai::agent::env::{ExecutionEnv, LocalExecutionEnv, LocalExecutionEnvOptions};
use std::{fs, path::PathBuf};

/// 创建唯一的临时测试目录。
fn temporary_directory() -> PathBuf {
    let directory = std::env::temp_dir().join(format!("xstudio-checkpoint-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&directory).expect("should create temporary directory");
    directory
}

/// 验证检查点仓库会忽略数据库、复用无变化提交并能强制回滚文件。
#[tokio::test]
async fn checkpoint_initializes_tracks_and_resets_workdir() {
    let directory = temporary_directory();
    fs::write(directory.join("xstudio.sqlite"), "ignored").expect("should write ignored database");
    fs::create_dir_all(directory.join("nested")).expect("should create nested directory");
    fs::write(directory.join("nested/db.sqlite"), "ignored").expect("should write nested ignored database");
    fs::write(directory.join("note.txt"), "initial").expect("should write tracked file");

    let env = LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: directory.clone(), ..Default::default() })
        .expect("should create local execution environment");
    assert_eq!(env.get_point().await.expect("should query empty checkpoint"), None);
    let initial = env.create_point().await.expect("should return initial checkpoint");
    assert!(directory.join(".xstudio/checkpoint.git").is_dir(), "should create bare repository");
    assert_eq!(env.get_point().await.expect("should query initial checkpoint"), Some(initial.clone()));
    assert_eq!(initial, env.create_point().await.expect("unchanged workdir should reuse checkpoint"));
    let reopened_env =
        LocalExecutionEnv::new(LocalExecutionEnvOptions { cwd: directory.clone(), ..Default::default() })
            .expect("should reopen local execution environment");
    assert_eq!(reopened_env.get_point().await.expect("should query persisted checkpoint"), Some(initial.clone()));

    fs::write(directory.join("note.txt"), "updated").expect("should update tracked file");
    fs::write(directory.join("created.txt"), "created").expect("should add tracked file");
    let updated = env.create_point().await.expect("changed workdir should create checkpoint");
    assert_ne!(initial, updated, "changed workdir should have a new checkpoint");

    env.reset_point(&initial).await.expect("should reset to initial checkpoint");
    assert_eq!(env.get_point().await.expect("should query restored checkpoint"), Some(initial));
    assert_eq!(fs::read_to_string(directory.join("note.txt")).expect("should read restored file"), "initial");
    assert!(!directory.join("created.txt").exists(), "should remove files added after the checkpoint");
    assert!(directory.join("xstudio.sqlite").exists(), "should retain ignored database");
    assert!(directory.join("nested/db.sqlite").exists(), "should retain nested ignored database");

    fs::remove_dir_all(directory).expect("should remove temporary directory");
}
