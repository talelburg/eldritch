//! Game lifecycle HTTP: create a game.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use game_core::scenario::ScenarioId;
use protocol::{CreateGameRequest, CreateGameResponse};

use crate::id::random_game_id;
use crate::session::{GameSession, SessionError};
use crate::AppState;

/// `POST /games`: set up a new game from a scenario and return its id.
///
/// `201 Created` with the `game_id` on success; `400 Bad Request` for an
/// unknown scenario; `422 Unprocessable Entity` for a roster the engine
/// rejects; `500` on a persistence failure.
pub(crate) async fn create_game(
    State(state): State<AppState>,
    Json(request): Json<CreateGameRequest>,
) -> Result<(StatusCode, Json<CreateGameResponse>), StatusCode> {
    let scenario_id = ScenarioId::new(request.scenario_id);
    match GameSession::create(
        state.db.clone(),
        random_game_id(),
        scenario_id,
        request.roster,
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
