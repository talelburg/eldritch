//! Game lifecycle HTTP: create a game.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use game_core::action::RosterEntry;
use game_core::scenario::ScenarioId;
use protocol::{CreateGameRequest, CreateGameResponse, GameId};

use crate::auth::SignedInAccount;
use crate::session::{self, GameSession, SessionError};
use crate::{id, lobby, AppState};

/// `POST /games`: create a game of a scenario and return its id. The
/// signed-in account is recorded as its creator.
///
/// With neither a roster nor a lead, the game is stored as a lobby, with the
/// creator as its first member. With both, the roster is seated at once and
/// the creator holds every investigator.
///
/// `201 Created` with the `game_id` on success; `401 Unauthorized` when not
/// signed in, whatever the body; `400 Bad Request` for an unknown scenario,
/// or for a roster without a lead or a lead without a roster;
/// `422 Unprocessable Entity` for a roster the engine rejects; `500` on a
/// persistence failure.
pub(crate) async fn create_game(
    State(state): State<AppState>,
    account: SignedInAccount,
    Json(request): Json<CreateGameRequest>,
) -> Result<(StatusCode, Json<CreateGameResponse>), StatusCode> {
    let game_id = match (request.roster, request.lead) {
        (None, None) => create_lobby(&state, &account, request.scenario_id).await?,
        (Some(roster), Some(lead)) => {
            seat(&state, &account, request.scenario_id, roster, lead).await?
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    Ok((StatusCode::CREATED, Json(CreateGameResponse { game_id })))
}

/// Store a lobby of `scenario_id` created by `account`.
async fn create_lobby(
    state: &AppState,
    account: &SignedInAccount,
    scenario_id: ScenarioId,
) -> Result<GameId, StatusCode> {
    // Nothing is seated until Start, so the scenario is checked here.
    if (state.registries.scenarios.module_for)(&scenario_id).is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let game_id = id::random_game_id();
    lobby::create(
        &state.db,
        &game_id,
        &scenario_id,
        account.id,
        session::unix_millis(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(game_id)
}

/// Seat `roster` led by `lead` into a new game of `scenario_id`, created by
/// `account`.
async fn seat(
    state: &AppState,
    account: &SignedInAccount,
    scenario_id: ScenarioId,
    roster: Vec<RosterEntry>,
    lead: usize,
) -> Result<GameId, StatusCode> {
    match GameSession::create(
        state.db.clone(),
        state.registries,
        id::random_game_id(),
        account.id,
        scenario_id,
        roster,
        lead,
    )
    .await
    {
        Ok(session) => Ok(session.game_id),
        Err(SessionError::UnknownScenario(_)) => Err(StatusCode::BAD_REQUEST),
        Err(SessionError::Seating(_)) => Err(StatusCode::UNPROCESSABLE_ENTITY),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
