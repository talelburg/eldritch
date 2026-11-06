//! A game's lobby, its members and its picks, as stored (ADR 0024).
//!
//! A lobby is a `lobbies` row, with its lead, and its picks' `lobby_picks`
//! rows, until Start deletes them in the transaction that stores the game.
//! Its members are `game_members` rows, which outlive Start: every account
//! that opens a game's link, lobby or started game, becomes a member. The
//! lobby's rules are [`protocol::lobby::Lobby`]'s; this module only moves its
//! rows in and out.
//!
//! `lobbies.created_at` is read back as a lobby's last activity by
//! `of_member`, whatever migration 0010's comment on the column says.

use cards::decks::WeaknessChoice;
use game_core::scenario::ScenarioId;
use game_core::state::CardCode;
use protocol::lobby::{Lobby, LobbyView, Member, Pick};
use protocol::{AccountId, GameId};
use sqlx::{Sqlite, SqliteConnection, SqlitePool};

/// Store a new lobby of `scenario_id` under `game_id`, created by `creator`
/// at `created_at` (unix millis), with the creator as its first member.
///
/// # Errors
///
/// The database failed, or `game_id` is taken.
pub(crate) async fn create(
    db: &SqlitePool,
    game_id: &GameId,
    scenario_id: &ScenarioId,
    creator: AccountId,
    created_at: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO lobbies (game_id, scenario_id, creator, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(game_id.as_str())
    .bind(scenario_id.as_str())
    .bind(creator.0)
    .bind(created_at)
    .execute(&mut *tx)
    .await?;
    add_member(&mut *tx, game_id, creator).await?;
    tx.commit().await
}

/// The lobby stored under `game_id`, with its members and their display
/// names, or `None` if no lobby is.
///
/// # Errors
///
/// The database failed, or a pick's stored weakness choice is neither one.
pub(crate) async fn read(db: &SqlitePool, game_id: &GameId) -> Result<Option<Lobby>, sqlx::Error> {
    let row: Option<(String, i64, Option<String>)> =
        sqlx::query_as("SELECT scenario_id, creator, lead FROM lobbies WHERE game_id = ?")
            .bind(game_id.as_str())
            .fetch_optional(db)
            .await?;
    let Some((scenario_id, creator, lead)) = row else {
        return Ok(None);
    };
    let members = named_members(db, game_id).await?;
    let picks: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT investigator, account_id, weakness FROM lobby_picks \
         WHERE game_id = ? ORDER BY position",
    )
    .bind(game_id.as_str())
    .fetch_all(db)
    .await?;
    let lobby = Lobby::new(ScenarioId::new(scenario_id), AccountId(creator), members).with_picks(
        picks
            .into_iter()
            .map(|(investigator, account, weakness)| {
                Ok(Pick {
                    investigator: CardCode::new(investigator),
                    account: AccountId(account),
                    weakness: weakness_from_column(&weakness)?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()?,
        lead.map(CardCode::new),
    );
    Ok(Some(lobby))
}

/// The members of the game or lobby `game_id`, with their display names, in
/// the order they joined.
///
/// # Errors
///
/// The database failed.
pub(crate) async fn named_members(
    db: &SqlitePool,
    game_id: &GameId,
) -> Result<Vec<Member>, sqlx::Error> {
    let members: Vec<(i64, String)> = sqlx::query_as(
        "SELECT m.account_id, a.display_name FROM game_members m \
         JOIN accounts a ON a.account_id = m.account_id \
         WHERE m.game_id = ? ORDER BY m.rowid",
    )
    .bind(game_id.as_str())
    .fetch_all(db)
    .await?;
    Ok(members
        .into_iter()
        .map(|(account, display_name)| Member {
            account: AccountId(account),
            display_name,
        })
        .collect())
}

/// Every lobby `account` is a member of, as its id, its view and its
/// `lobbies.created_at` (unix millis), which is its last activity on the
/// "Your games" listing.
///
/// # Errors
///
/// The database failed, or a lobby's row doesn't read back as [`read`] says.
pub(crate) async fn of_member(
    db: &SqlitePool,
    account: AccountId,
) -> Result<Vec<(GameId, LobbyView, i64)>, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT l.game_id, l.created_at FROM game_members m \
         JOIN lobbies l ON l.game_id = m.game_id WHERE m.account_id = ?",
    )
    .bind(account.0)
    .fetch_all(db)
    .await?;
    let mut lobbies = Vec::with_capacity(rows.len());
    for (game_id, created_at) in rows {
        let game_id = GameId::new(game_id);
        // A lobby Start deleted since the query above is a started game now.
        if let Some(lobby) = read(db, &game_id).await? {
            lobbies.push((game_id, lobby.view(), created_at));
        }
    }
    Ok(lobbies)
}

/// Store `view`'s picks, in its seating order, and its lead as the lobby
/// `game_id`'s, in place of those stored, on `db`: a transaction the caller
/// commits, so a change of membership can be stored with them.
///
/// # Errors
///
/// The database failed, or the view picks one investigator twice.
pub(crate) async fn save_picks(
    db: &mut SqliteConnection,
    game_id: &GameId,
    view: &LobbyView,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM lobby_picks WHERE game_id = ?")
        .bind(game_id.as_str())
        .execute(&mut *db)
        .await?;
    for (position, pick) in (0_i64..).zip(&view.picks) {
        sqlx::query(
            "INSERT INTO lobby_picks (game_id, investigator, account_id, weakness, position) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(game_id.as_str())
        .bind(pick.investigator.as_str())
        .bind(pick.account.0)
        .bind(weakness_column(pick.weakness))
        .bind(position)
        .execute(&mut *db)
        .await?;
    }
    sqlx::query("UPDATE lobbies SET lead = ? WHERE game_id = ?")
        .bind(view.lead.as_ref().map(CardCode::as_str))
        .bind(game_id.as_str())
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Make `account` no longer a member of the game `game_id`, on `db`. Opening
/// the game's link again makes it one again, through [`add_member`].
///
/// # Errors
///
/// The database failed.
pub(crate) async fn remove_member(
    db: &mut SqliteConnection,
    game_id: &GameId,
    account: AccountId,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM game_members WHERE game_id = ? AND account_id = ?")
        .bind(game_id.as_str())
        .bind(account.0)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Delete the lobby `game_id`'s rows, its picks' and its own, on `db`: what
/// Start does in the transaction that stores the game. Its members' rows
/// stay, since membership outlives Start.
///
/// # Errors
///
/// The database failed.
pub(crate) async fn delete(db: &mut SqliteConnection, game_id: &GameId) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM lobby_picks WHERE game_id = ?")
        .bind(game_id.as_str())
        .execute(&mut *db)
        .await?;
    sqlx::query("DELETE FROM lobbies WHERE game_id = ?")
        .bind(game_id.as_str())
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// A pick's weakness choice as its `lobby_picks.weakness` column holds it.
fn weakness_column(choice: WeaknessChoice) -> &'static str {
    match choice {
        WeaknessChoice::Fixed => "fixed",
        WeaknessChoice::Random => "random",
    }
}

/// The weakness choice a `lobby_picks.weakness` column holds.
///
/// # Errors
///
/// The column holds neither value [`weakness_column`] writes.
fn weakness_from_column(column: &str) -> Result<WeaknessChoice, sqlx::Error> {
    match column {
        "fixed" => Ok(WeaknessChoice::Fixed),
        "random" => Ok(WeaknessChoice::Random),
        other => Err(sqlx::Error::Decode(
            format!("lobby_picks.weakness holds {other:?}").into(),
        )),
    }
}

/// Make `account` a member of the game `game_id`, if it isn't one. Returns
/// whether this made it one.
///
/// # Errors
///
/// The database failed.
pub(crate) async fn add_member(
    db: impl sqlx::Executor<'_, Database = Sqlite>,
    game_id: &GameId,
    account: AccountId,
) -> Result<bool, sqlx::Error> {
    let inserted =
        sqlx::query("INSERT OR IGNORE INTO game_members (game_id, account_id) VALUES (?, ?)")
            .bind(game_id.as_str())
            .bind(account.0)
            .execute(db)
            .await?;
    Ok(inserted.rows_affected() == 1)
}

/// The members of the game `game_id`, lobby or started, in the order they
/// joined.
///
/// # Errors
///
/// The database failed.
pub async fn members_of(db: &SqlitePool, game_id: &GameId) -> Result<Vec<AccountId>, sqlx::Error> {
    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT account_id FROM game_members WHERE game_id = ? ORDER BY rowid")
            .bind(game_id.as_str())
            .fetch_all(db)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(account,)| AccountId(account))
        .collect())
}
