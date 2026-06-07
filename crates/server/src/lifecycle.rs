//! Game lifecycle HTTP: create a game.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use game_core::scenario::ScenarioId;
use serde::{Deserialize, Serialize};

use crate::id::GameId;
use crate::session::{GameSession, SessionError};
use crate::AppState;

/// Body of `POST /games`.
#[derive(Debug, Deserialize)]
pub struct CreateGameRequest {
    /// The scenario module to set up.
    pub scenario_id: String,
}

/// Response to a successful `POST /games`.
#[derive(Debug, Serialize)]
pub struct CreateGameResponse {
    /// The newly created game's id.
    pub game_id: GameId,
}

/// `POST /games`: set up a new game from a scenario and return its id.
///
/// `201 Created` with the `game_id` on success; `400 Bad Request` for an
/// unknown scenario; `500` on a persistence failure.
pub(crate) async fn create_game(
    State(state): State<AppState>,
    Json(request): Json<CreateGameRequest>,
) -> Result<(StatusCode, Json<CreateGameResponse>), StatusCode> {
    let scenario_id = ScenarioId::new(request.scenario_id);
    match GameSession::create(state.db.clone(), GameId::random(), scenario_id).await {
        Ok(session) => Ok((
            StatusCode::CREATED,
            Json(CreateGameResponse {
                game_id: session.game_id,
            }),
        )),
        Err(SessionError::UnknownScenario(_)) => Err(StatusCode::BAD_REQUEST),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
