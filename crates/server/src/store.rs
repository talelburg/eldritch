//! Action-log persistence: thin CRUD over the `games` and `actions`
//! tables. JSON (de)serialization lives in [`crate::session`]; this
//! layer only moves strings in and out of `SQLite`.

use sqlx::SqlitePool;

/// Insert a new game's seed row.
pub(crate) async fn insert_game(
    db: &SqlitePool,
    game_id: &str,
    scenario_id: &str,
    seed_state: &str,
    created_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO games (game_id, scenario_id, seed_state, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(game_id)
    .bind(scenario_id)
    .bind(seed_state)
    .bind(created_at)
    .execute(db)
    .await?;
    Ok(())
}

/// Append one action row at `seq` for a game.
pub(crate) async fn insert_action(
    db: &SqlitePool,
    game_id: &str,
    seq: i64,
    action: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO actions (game_id, seq, action) VALUES (?, ?, ?)")
        .bind(game_id)
        .bind(seq)
        .bind(action)
        .execute(db)
        .await?;
    Ok(())
}

/// Fetch a game's `(scenario_id, seed_state)`, or `None` if no such game.
pub(crate) async fn load_game(
    db: &SqlitePool,
    game_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as("SELECT scenario_id, seed_state FROM games WHERE game_id = ?")
        .bind(game_id)
        .fetch_optional(db)
        .await
}

/// Fetch a game's action JSON blobs in `seq` order.
pub(crate) async fn load_actions(
    db: &SqlitePool,
    game_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT action FROM actions WHERE game_id = ? ORDER BY seq")
            .bind(game_id)
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().map(|(action,)| action).collect())
}
