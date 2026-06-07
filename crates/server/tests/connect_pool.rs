//! `connect_pool` opens a pool and creates the database file if it does
//! not already exist (so first boot on a clean host just works).

use std::path::PathBuf;

fn unique_db_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("eldritch-connect-pool-{nanos}.db"))
}

#[tokio::test]
async fn connect_pool_creates_database_file_if_missing() {
    let path = unique_db_path();
    assert!(!path.exists(), "precondition: db file must not pre-exist");

    let url = format!("sqlite:{}", path.display());
    let pool = server::db::connect_pool(&url)
        .await
        .expect("connect_pool should open (and create) the database");

    assert!(path.exists(), "connect_pool should have created the file");

    // The returned pool is usable.
    let (one,): (i64,) = sqlx::query_as("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("trivial query on the new pool");
    assert_eq!(one, 1);

    pool.close().await;
    let _ = std::fs::remove_file(&path);
}
