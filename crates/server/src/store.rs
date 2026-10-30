//! Action-log persistence: thin CRUD over the `games` and `actions`
//! tables. JSON (de)serialization lives in [`crate::session`]; this
//! layer only moves strings and integers in and out of `SQLite`.

use protocol::GameId;
use sqlx::{Error, SqlitePool};

/// What a `games` row holds to rebuild its game: the creation inputs, in their
/// column form, and the record of what setup decided. No serialized engine
/// state (ADR 0022). The row's diagnostic columns, `created_build` and
/// `created_at`, are written beside it and never read back.
pub(crate) struct GameRow {
    /// The stored shape's version; only the current one loads.
    pub(crate) format_version: i64,
    pub(crate) scenario_id: String,
    /// JSON array of the roster entries the game was seated with.
    pub(crate) roster: String,
    pub(crate) lead: i64,
    /// The `u64` setup seed's bits, as `SQLite`'s signed 64-bit integer.
    pub(crate) rng_seed: i64,
    pub(crate) interactive_acknowledge: bool,
    /// JSON of the setup fingerprint.
    pub(crate) setup_fingerprint: String,
}

/// Insert a new game's row, created by build `created_build` at `created_at`
/// (unix millis).
pub(crate) async fn insert_game(
    db: &SqlitePool,
    game_id: &GameId,
    row: &GameRow,
    created_build: &str,
    created_at: i64,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO games (game_id, format_version, created_build, scenario_id, roster, lead, \
         rng_seed, interactive_acknowledge, setup_fingerprint, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(game_id.as_str())
    .bind(row.format_version)
    .bind(created_build)
    .bind(&row.scenario_id)
    .bind(&row.roster)
    .bind(row.lead)
    .bind(row.rng_seed)
    .bind(row.interactive_acknowledge)
    .bind(&row.setup_fingerprint)
    .bind(created_at)
    .execute(db)
    .await?;
    Ok(())
}

/// What an `actions` row holds for replay: the logged answer and the RNG
/// checkpoint after it.
pub(crate) struct ActionRow {
    /// JSON of the logged answer.
    pub(crate) answer: String,
    /// `RngState::draws` after the action applied, its `u64` bits as
    /// `SQLite`'s signed 64-bit integer.
    pub(crate) draws: i64,
}

/// Append one action row at `seq` for a game, applied at `at` (unix millis).
pub(crate) async fn insert_action(
    db: &SqlitePool,
    game_id: &GameId,
    seq: i64,
    row: &ActionRow,
    at: i64,
) -> Result<(), Error> {
    sqlx::query("INSERT INTO actions (game_id, seq, answer, draws, at) VALUES (?, ?, ?, ?, ?)")
        .bind(game_id.as_str())
        .bind(seq)
        .bind(&row.answer)
        .bind(row.draws)
        .bind(at)
        .execute(db)
        .await?;
    Ok(())
}

/// Fetch a game's row, or `None`.
pub(crate) async fn load_game(db: &SqlitePool, game_id: &GameId) -> Result<Option<GameRow>, Error> {
    let row: Option<(i64, String, String, i64, i64, bool, String)> = sqlx::query_as(
        "SELECT format_version, scenario_id, roster, lead, rng_seed, interactive_acknowledge, \
         setup_fingerprint FROM games WHERE game_id = ?",
    )
    .bind(game_id.as_str())
    .fetch_optional(db)
    .await?;
    Ok(row.map(
        |(
            format_version,
            scenario_id,
            roster,
            lead,
            rng_seed,
            interactive_acknowledge,
            setup_fingerprint,
        )| GameRow {
            format_version,
            scenario_id,
            roster,
            lead,
            rng_seed,
            interactive_acknowledge,
            setup_fingerprint,
        },
    ))
}

/// Fetch a game's action rows in `seq` order.
pub(crate) async fn load_actions(
    db: &SqlitePool,
    game_id: &GameId,
) -> Result<Vec<ActionRow>, Error> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT answer, draws FROM actions WHERE game_id = ? ORDER BY seq")
            .bind(game_id.as_str())
            .fetch_all(db)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(answer, draws)| ActionRow { answer, draws })
        .collect())
}
