//! Game lifecycle HTTP: create a game.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use protocol::{CreateGameRequest, CreateGameResponse};

use crate::session::{GameSession, SessionError};
use crate::{id, AppState};

/// `POST /games`: set up a new game from a scenario and return its id.
///
/// `201 Created` with the `game_id` on success; `400 Bad Request` for an
/// unknown scenario; `422 Unprocessable Entity` for a roster the engine
/// rejects; `500` on a persistence failure.
pub(crate) async fn create_game(
    State(state): State<AppState>,
    Json(request): Json<CreateGameRequest>,
) -> Result<(StatusCode, Json<CreateGameResponse>), StatusCode> {
    match GameSession::create(
        state.db.clone(),
        state.registries,
        id::random_game_id(),
        request.scenario_id,
        request.roster,
        request.lead,
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
