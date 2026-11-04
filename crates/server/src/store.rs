//! Action-log persistence: thin CRUD over the `games`, `game_assignments`
//! and `actions` tables. JSON (de)serialization lives in
//! [`crate::session`]; this layer only moves strings and integers in and out
//! of `SQLite`.

use game_core::state::InvestigatorId;
use protocol::{AccountId, Assignment, GameId};
use sqlx::{Error, SqliteConnection, SqlitePool};

/// What a `games` row holds to rebuild its game: the creation inputs, in their
/// column form, the record of what setup decided, and the account that created
/// it. No serialized engine state (ADR 0022), and not who holds each
/// investigator, which is stored beside it (ADR 0024). The row's
/// `created_build` and `created_at` are written beside it, and
/// [`started_games_of`] reads both back: `created_at` as a game's last
/// activity before any answer is logged, and `created_build` as the build an
/// unresumable game started on.
pub(crate) struct GameRow {
    /// The stored shape's version; only the current one loads.
    pub(crate) format_version: i64,
    /// The creating account. Every row this build stores has one; a row stored
    /// before the column existed has none.
    pub(crate) creator: Option<AccountId>,
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
    db: &mut SqliteConnection,
    game_id: &GameId,
    row: &GameRow,
    created_build: &str,
    created_at: i64,
) -> Result<(), Error> {
    sqlx::query(
        "INSERT INTO games (game_id, format_version, created_build, creator, scenario_id, roster, \
         lead, rng_seed, interactive_acknowledge, setup_fingerprint, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(game_id.as_str())
    .bind(row.format_version)
    .bind(created_build)
    .bind(row.creator.map(|creator| creator.0))
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

/// Insert a game's assignment, one row per investigator. The game's row must
/// already be inserted.
pub(crate) async fn insert_assignment(
    db: &mut SqliteConnection,
    game_id: &GameId,
    assignment: &Assignment,
) -> Result<(), Error> {
    for (investigator, account) in assignment.iter() {
        sqlx::query(
            "INSERT INTO game_assignments (game_id, investigator, account_id) VALUES (?, ?, ?)",
        )
        .bind(game_id.as_str())
        .bind(investigator.0)
        .bind(account.0)
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

/// Fetch a game's assignment: empty if it has no assignment rows.
pub(crate) async fn load_assignment(
    db: &SqlitePool,
    game_id: &GameId,
) -> Result<Assignment, Error> {
    let rows: Vec<(u32, i64)> =
        sqlx::query_as("SELECT investigator, account_id FROM game_assignments WHERE game_id = ?")
            .bind(game_id.as_str())
            .fetch_all(db)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(investigator, account)| (InvestigatorId(investigator), AccountId(account)))
        .collect())
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

/// A started game an account is a member of, as "Your games" lists it.
pub(crate) struct StartedGame {
    pub(crate) game_id: GameId,
    pub(crate) scenario_id: String,
    /// The creating account; `None` for a row stored before the column
    /// existed.
    pub(crate) creator: Option<AccountId>,
    /// When the game last moved, in unix millis: its last logged answer's
    /// `at`, or its `created_at` with none logged.
    pub(crate) last_activity: i64,
    /// The build that created it.
    pub(crate) created_build: String,
    /// JSON array of the roster entries it was seated with, as stored: in
    /// another stored shape for a game of another format version.
    pub(crate) roster: String,
    /// The roster index of its lead investigator.
    pub(crate) lead: i64,
    /// How many answers its log holds.
    pub(crate) logged_answers: u32,
}

/// The columns [`started_games_of`] selects, in order.
type StartedColumns = (String, String, Option<i64>, i64, String, String, i64, i64);

/// Every started game `account` is a member of: each with a `games` row.
pub(crate) async fn started_games_of(
    db: &SqlitePool,
    account: AccountId,
) -> Result<Vec<StartedGame>, Error> {
    let rows: Vec<StartedColumns> = sqlx::query_as(
        "SELECT g.game_id, g.scenario_id, g.creator, \
         COALESCE((SELECT MAX(a.at) FROM actions a WHERE a.game_id = g.game_id), g.created_at), \
         g.created_build, g.roster, g.lead, \
         (SELECT COUNT(*) FROM actions a WHERE a.game_id = g.game_id) \
         FROM game_members m JOIN games g ON g.game_id = m.game_id WHERE m.account_id = ?",
    )
    .bind(account.0)
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(
            |(
                game_id,
                scenario_id,
                creator,
                last_activity,
                created_build,
                roster,
                lead,
                logged_answers,
            )| {
                Ok(StartedGame {
                    game_id: GameId::new(game_id),
                    scenario_id,
                    creator: creator.map(AccountId),
                    last_activity,
                    created_build,
                    roster,
                    lead,
                    logged_answers: u32::try_from(logged_answers)
                        .map_err(|err| Error::Decode(Box::new(err)))?,
                })
            },
        )
        .collect()
}

/// The columns [`load_game`] selects, in order.
type GameColumns = (i64, Option<i64>, String, String, i64, i64, bool, String);

/// Fetch a game's row, or `None`.
pub(crate) async fn load_game(db: &SqlitePool, game_id: &GameId) -> Result<Option<GameRow>, Error> {
    let row: Option<GameColumns> = sqlx::query_as(
        "SELECT format_version, creator, scenario_id, roster, lead, rng_seed, \
         interactive_acknowledge, setup_fingerprint FROM games WHERE game_id = ?",
    )
    .bind(game_id.as_str())
    .fetch_optional(db)
    .await?;
    Ok(row.map(
        |(
            format_version,
            creator,
            scenario_id,
            roster,
            lead,
            rng_seed,
            interactive_acknowledge,
            setup_fingerprint,
        )| GameRow {
            format_version,
            creator: creator.map(AccountId),
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
