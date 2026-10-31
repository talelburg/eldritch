//! `connect_pool` opens a pool and creates the database file if it does
//! not already exist (so first boot on a clean host just works).

use server::db;

use crate::common::TempDb;

#[tokio::test]
async fn connect_pool_creates_database_file_if_missing() {
    let db_file = TempDb::new("connect-pool");
    assert!(
        !db_file.path().exists(),
        "precondition: db file must not pre-exist"
    );

    let pool = db::connect_pool(&db_file.url())
        .await
        .expect("connect_pool should open (and create) the database");

    assert!(
        db_file.path().exists(),
        "connect_pool should have created the file"
    );

    // The returned pool is usable.
    let (one,): (i64,) = sqlx::query_as("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("trivial query on the new pool");
    assert_eq!(one, 1);

    pool.close().await;
}
