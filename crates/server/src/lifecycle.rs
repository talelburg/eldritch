//! Game lifecycle HTTP: create a game, and list the signed-in account's games.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use game_core::scenario::ScenarioId;
use protocol::{AccountId, CreateGameRequest, CreateGameResponse, GameEntry};

use crate::auth::SignedInAccount;
use crate::room::RoomGame;
use crate::rooms::LoadFailure;
use crate::store::{self, StartedGame};
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
/// A started game that fails to load is left out.
///
/// `200 OK` with the entries; `401 Unauthorized` when not signed in; `500` on
/// a persistence failure.
pub(crate) async fn list_games(
    State(state): State<AppState>,
    account: SignedInAccount,
) -> Result<Json<Vec<GameEntry>>, StatusCode> {
    let failed = |_| StatusCode::INTERNAL_SERVER_ERROR;
    let lobbies = lobby::of_member(&state.db, account.id)
        .await
        .map_err(failed)?;
    let mut entries: Vec<GameEntry> = lobbies
        .into_iter()
        .map(|(game_id, view, created_at)| GameEntry::of_lobby(game_id, &view, created_at))
        .collect();
    for game in store::started_games_of(&state.db, account.id)
        .await
        .map_err(failed)?
    {
        if let Some(entry) = started_entry(&state, account.id, game).await? {
            entries.push(entry);
        }
    }
    Ok(Json(entries))
}

/// The entry `account` is shown for the started game `game`, loaded through
/// the room registry; `None` if it isn't live.
///
/// # Errors
///
/// `500` if its members can't be read.
async fn started_entry(
    state: &AppState,
    account: AccountId,
    game: StartedGame,
) -> Result<Option<GameEntry>, StatusCode> {
    // Every row this build stores names its creator; one that doesn't was
    // stored by an older server.
    let Some(creator) = game.creator else {
        return Ok(None);
    };
    let connection = match state.rooms.get_or_load(&game.game_id).await {
        Ok(connection) => connection,
        // Gone since the query above.
        Err(LoadFailure::NotFound) => return Ok(None),
        Err(LoadFailure::Failed(err)) => {
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let room = connection.room().game.lock().await;
    let RoomGame::Live(session) = &*room else {
        return Ok(None);
    };
    Ok(Some(GameEntry::of_live(
        game.game_id,
        ScenarioId::new(game.scenario_id),
        creator,
        session.live(),
        account,
        &members,
        game.last_activity,
    )))
}
