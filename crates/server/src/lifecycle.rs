//! Game lifecycle HTTP: create a game.

use std::iter;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use protocol::{Assignment, CreateGameRequest, CreateGameResponse};

use crate::auth::SignedInAccount;
use crate::session::{GameSession, SessionError};
use crate::{id, AppState};

/// `POST /games`: set up a new game from a scenario and return its id. The
/// signed-in account is recorded as its creator, and is assigned every
/// investigator.
///
/// `201 Created` with the `game_id` on success; `401 Unauthorized` when not
/// signed in, whatever the body; `400 Bad Request` for an unknown scenario;
/// `422 Unprocessable Entity` for a roster the engine rejects; `500` on a
/// persistence failure.
pub(crate) async fn create_game(
    State(state): State<AppState>,
    account: SignedInAccount,
    Json(request): Json<CreateGameRequest>,
) -> Result<(StatusCode, Json<CreateGameResponse>), StatusCode> {
    let assignment = Assignment::in_seating_order(iter::repeat_n(account.id, request.roster.len()));
    match GameSession::create(
        state.db.clone(),
        state.registries,
        id::random_game_id(),
        account.id,
        request.scenario_id,
        request.roster,
        request.lead,
        assignment,
    )
    .await
    {
        Ok(session) => Ok((
            StatusCode::CREATED,
            Json(CreateGameResponse {
                game_id: session.game_id,
            }),
        )),
        Err(SessionError::UnknownScenario(_)) => Err(StatusCode::BAD_REQUEST),
        Err(SessionError::Seating(_)) => Err(StatusCode::UNPROCESSABLE_ENTITY),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
