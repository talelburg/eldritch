//! Game lifecycle HTTP: create a game.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use protocol::{CreateGameRequest, CreateGameResponse};

use crate::auth::SignedInAccount;
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
