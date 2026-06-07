//! Eldritch server library: HTTP/WS surface + `SQLite` persistence.
//!
//! The binary (`main.rs`) is a thin wrapper that wires configuration,
//! opens the database, and serves the router built here. Keeping the
//! logic in a library target lets integration tests in `tests/` drive
//! it directly.

pub mod db;
mod id;
pub mod lifecycle;
pub mod session;
mod store;
mod ws;

pub use id::GameId;
pub use session::{GameSession, SessionError};

use std::path::PathBuf;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};

/// Shared application state handed to every Axum handler.
#[derive(Clone)]
pub struct AppState {
    /// Connection pool for the `SQLite` action-log database.
    pub db: SqlitePool,
    /// Live games keyed by `game_id`, each with its broadcast group.
    rooms: ws::Rooms,
    /// Directory holding the built client bundle (`index.html`, JS, wasm),
    /// served as the router fallback.
    dist_dir: PathBuf,
}

impl AppState {
    /// Build application state over a database pool, serving the client
    /// bundle from the default dev location (`crates/web/dist`, relative
    /// to the workspace root).
    #[must_use]
    pub fn new(db: SqlitePool) -> Self {
        Self::new_with_dist(db, PathBuf::from("crates/web/dist"))
    }

    /// Build application state with an explicit client-bundle directory.
    #[must_use]
    pub fn new_with_dist(db: SqlitePool, dist_dir: PathBuf) -> Self {
        Self {
            db,
            rooms: ws::rooms(),
            dist_dir,
        }
    }
}

/// Install the process-global scenario + card registries the server
/// needs to create and play games. Call once at startup.
///
/// Phase 6 installs the **synthetic** fixtures (the toy scenario and
/// its `_synth_*` cards) knowingly — the only playable content this
/// phase. Idempotent: a second call is a no-op (the underlying
/// `OnceLock`s reject re-installation).
///
/// TODO(phase-7): swap to the real `scenarios`/`cards` registries when
/// The Gathering lands.
pub fn install_registries() {
    let _ = game_core::scenario_registry::install(scenarios::REGISTRY);
    let _ = game_core::card_registry::install(scenarios::test_fixtures::synth_cards::TEST_REGISTRY);
}

/// Build the application router with all routes and shared state. The
/// JSON API and WebSocket take precedence; everything else falls back
/// to the client bundle, with `index.html` as the SPA fallback.
pub fn app(state: AppState) -> Router {
    let index_html = state.dist_dir.join("index.html");
    let static_files = ServeDir::new(&state.dist_dir).fallback(ServeFile::new(index_html));

    Router::new()
        .route("/health", get(health))
        .route("/games", post(lifecycle::create_game))
        .route("/games/{game_id}/ws", get(ws::game_ws))
        .fallback_service(static_files)
        .with_state(state)
}

/// Readiness probe: `200 OK` if the database answers a trivial query,
/// `503 Service Unavailable` otherwise.
async fn health(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
