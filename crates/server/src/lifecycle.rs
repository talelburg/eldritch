//! Game lifecycle HTTP: create a game, list the signed-in account's games,
//! and delete one.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use game_core::scenario::ScenarioId;
use game_core::state::{CardCode, InvestigatorId};
use protocol::lobby::Member;
use protocol::{
    AccountId, Assignment, CreateGameRequest, CreateGameResponse, EntryInvestigator, GameEntry,
    GameId, ListedGame, UnresumableSummary,
};
use serde_json::Value;

use crate::auth::SignedInAccount;
use crate::room::{DeleteError, RoomGame};
use crate::rooms::LoadFailure;
use crate::session::{SessionError, UnresumableReason};
use crate::store::{self, StartedGame, StoredCreator};
use crate::{id, lobby, session, AppState};

/// `POST /games`: create a game of a scenario as a lobby, with the signed-in
/// account as its creator and first member, and return its id. Nothing is
/// seated until the creator presses Start in the lobby.
///
/// `201 Created` with the `game_id` on success; `401 Unauthorized` when not
/// signed in, whatever the body; `400 Bad Request` for an unknown scenario;
/// `500` on a persistence failure.
pub(crate) async fn create_game(
    State(state): State<AppState>,
    account: SignedInAccount,
    Json(request): Json<CreateGameRequest>,
) -> Result<(StatusCode, Json<CreateGameResponse>), StatusCode> {
    // Nothing is seated until Start, so the scenario is checked here.
    if (state.registries.scenarios.module_for)(&request.scenario_id).is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let game_id = id::random_game_id();
    lobby::create(
        &state.db,
        &game_id,
        &request.scenario_id,
        account.id,
        session::unix_millis(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(CreateGameResponse { game_id })))
}

/// `GET /games`: the "Your games" listing, one [`GameEntry`] per game the
/// signed-in account is a member of, in no particular order.
///
/// A started game is loaded through the room registry, as a socket's attach
/// loads it, and summarised from the live game: nothing derived from its
/// state is stored (ADR 0022). A loaded room stays for its usual idle grace.
/// A started game this build can't replay is listed as unresumable, from its
/// stored rows; one that fails to load for another reason, such as a
/// database error, is left out and logged, and the next listing tries again.
///
/// `200 OK` with the entries; `401 Unauthorized` when not signed in; `500` on
/// a persistence failure.
pub(crate) async fn list_games(
    State(state): State<AppState>,
    account: SignedInAccount,
) -> Result<Json<Vec<GameEntry>>, StatusCode> {
    let lobbies = lobby::of_member(&state.db, account.id)
        .await
        .map_err(internal_error)?;
    let mut entries: Vec<GameEntry> = lobbies
        .into_iter()
        .map(|(game_id, view, created_at)| GameEntry::of_lobby(game_id, &view, created_at))
        .collect();
    for game in store::started_games_of(&state.db, account.id)
        .await
        .map_err(internal_error)?
    {
        if let Some(entry) = started_entry(&state, account.id, game).await? {
            entries.push(entry);
        }
    }
    Ok(Json(entries))
}

/// `DELETE /games/{game_id}`: the creator deletes the game, lobby or started,
/// and every row stored under it.
///
/// The game is got through the room registry, as a socket gets it. A room is
/// deleted under its lock, which closes every connection to it, and is then
/// evicted. An unresumable game has no room: its rows are deleted through the
/// registry, which holds the game's place while they go, so a connection
/// arriving meanwhile waits for the delete. Either way, a later connection
/// finds no game.
/// The creator is read from the lobby, the live game, or for an unresumable
/// game its stored row.
///
/// `204 No Content` on success; `401 Unauthorized` when not signed in; `403
/// Forbidden` for any account but the creator, deleting nothing; `404 Not
/// Found` for no such game; `500` on a persistence failure.
pub(crate) async fn delete_game(
    State(state): State<AppState>,
    account: SignedInAccount,
    Path(game_id): Path<GameId>,
) -> StatusCode {
    let deleted = match state.rooms.get_or_load(&game_id).await {
        Ok(connection) => {
            let deleted = connection
                .room()
                .delete(account.id)
                .await
                .map_err(|error| match error {
                    DeleteError::NotCreator => StatusCode::FORBIDDEN,
                    DeleteError::Deleted => StatusCode::NOT_FOUND,
                    DeleteError::Db(error) => internal_error(error),
                });
            if deleted.is_ok() {
                state.rooms.forget(&game_id);
            }
            deleted
        }
        Err(LoadFailure::NotFound) => Err(StatusCode::NOT_FOUND),
        Err(LoadFailure::Failed(err)) if matches!(*err, SessionError::Unresumable { .. }) => {
            state
                .rooms
                .delete_with(&game_id, delete_unresumable(&state, &game_id, account.id))
                .await
        }
        Err(LoadFailure::Failed(err)) => {
            tracing::error!(
                game_id = game_id.as_str(),
                error = %err,
                "refusing to delete: this game failed to load",
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    };
    match deleted {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(status) => status,
    }
}

/// Delete the unresumable game `game_id`'s rows if `account` created it, as
/// its stored row says. A row with no creator can't be deleted by anyone.
///
/// # Errors
///
/// `403` for any other account, `404` if the rows are gone, `500` on a
/// persistence failure.
async fn delete_unresumable(
    state: &AppState,
    game_id: &GameId,
    account: AccountId,
) -> Result<(), StatusCode> {
    match store::creator_of(&state.db, game_id)
        .await
        .map_err(internal_error)?
    {
        StoredCreator::NoGame => Err(StatusCode::NOT_FOUND),
        StoredCreator::Account(creator) if creator == account => {
            store::delete_game(&state.db, game_id)
                .await
                .map_err(internal_error)
        }
        StoredCreator::Account(_) | StoredCreator::Unrecorded => Err(StatusCode::FORBIDDEN),
    }
}

/// The entry `account` is shown for the started game `game`, loaded through
/// the room registry: live, or unresumable; `None` if it is neither.
///
/// # Errors
///
/// `500` if its members or its assignment can't be read.
async fn started_entry(
    state: &AppState,
    account: AccountId,
    game: StartedGame,
) -> Result<Option<GameEntry>, StatusCode> {
    let connection = match state.rooms.get_or_load(&game.game_id).await {
        Ok(connection) => connection,
        // Gone since the query above.
        Err(LoadFailure::NotFound) => return Ok(None),
        Err(LoadFailure::Failed(err)) => {
            if let SessionError::Unresumable { reason, .. } = &*err {
                return unresumable_entry(state, game, reason).await.map(Some);
            }
            tracing::error!(
                game_id = game.game_id.as_str(),
                error = %err,
                "leaving a game out of the listing: it failed to load",
            );
            return Ok(None);
        }
    };
    let members = lobby::named_members(&state.db, &game.game_id)
        .await
        .map_err(internal_error)?;
    let room = connection.room().game.lock().await;
    let RoomGame::Live(session) = &*room else {
        return Ok(None);
    };
    Ok(Some(GameEntry::of_live(
        listed(game, members),
        session.live(),
        account,
    )))
}

/// The entry for the started game `game`, which this build can't replay for
/// `reason`. Its investigators come from its stored roster and assignment,
/// so the row reads like any other.
///
/// # Errors
///
/// `500` if its members or its assignment can't be read.
async fn unresumable_entry(
    state: &AppState,
    game: StartedGame,
    reason: &UnresumableReason,
) -> Result<GameEntry, StatusCode> {
    let members = lobby::named_members(&state.db, &game.game_id)
        .await
        .map_err(internal_error)?;
    let assignment = store::load_assignment(&state.db, &game.game_id)
        .await
        .map_err(internal_error)?;
    let investigators = stored_investigators(&game.roster, game.lead, &assignment);
    let summary = UnresumableSummary {
        reason: reason.cause(),
        logged_answers: game.logged_answers,
        created_build: game.created_build.clone(),
        server_build: protocol::build_id().to_owned(),
    };
    Ok(GameEntry::of_unresumable(
        listed(game, members),
        investigators,
        summary,
    ))
}

/// The started game `game` as its entry lists it, with its `members`.
fn listed(game: StartedGame, members: Vec<Member>) -> ListedGame {
    ListedGame {
        game_id: game.game_id,
        scenario_id: ScenarioId::new(game.scenario_id),
        creator: game.creator,
        members,
        last_activity: game.last_activity,
    }
}

/// The `500` a listing answers a persistence failure with.
fn internal_error<E>(_: E) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

/// The investigators of the stored `roster` JSON, in seating order, the one
/// at roster index `lead` leading, each held as `assignment` says.
///
/// Read leniently, since an unresumable game may be stored in another shape:
/// each roster entry's `investigator` field, and no investigators at all if
/// the roster isn't an array of entries carrying one.
fn stored_investigators(
    roster: &str,
    lead: i64,
    assignment: &Assignment,
) -> Vec<EntryInvestigator> {
    let Ok(Value::Array(entries)) = serde_json::from_str(roster) else {
        return Vec::new();
    };
    let codes: Option<Vec<&str>> = entries
        .iter()
        .map(|entry| entry.get("investigator")?.as_str())
        .collect();
    // Seating gives the entry at roster index `i` `InvestigatorId(i + 1)`.
    (1..)
        .zip(codes.unwrap_or_default())
        .map(|(id, code)| EntryInvestigator {
            investigator: CardCode::new(code),
            holder: assignment.holder(InvestigatorId(id)),
            lead: i64::from(id) - 1 == lead,
        })
        .collect()
}
