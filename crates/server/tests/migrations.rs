//! Schema-migration smoke test: a fresh database brought up by the
//! embedded migrator has the action-log tables.

use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn migrations_create_games_and_actions_tables() {
    // A single-connection in-memory pool keeps the whole database on one
    // connection, so migrations run on the same database the assertion
    // queries (default `sqlite::memory:` gives each connection its own).
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");

    server::db::MIGRATOR
        .run(&pool)
        .await
        .expect("run migrations");

    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'table' AND name IN ('games', 'actions')",
    )
    .fetch_one(&pool)
    .await
    .expect("query sqlite_master");

    assert_eq!(count, 2, "both games and actions tables should exist");
}
