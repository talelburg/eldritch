//! Eldritch server library: HTTP/WS surface + `SQLite` persistence.
//!
//! The binary (`main.rs`) is a thin wrapper that wires configuration,
//! opens the database, and serves the router built here. Keeping the
//! logic in a library target lets integration tests in `tests/` drive
//! it directly.

pub mod db;
pub mod session;
mod store;

pub use session::{GameSession, SessionError};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use sqlx::SqlitePool;

/// Shared application state handed to every Axum handler.
#[derive(Clone)]
pub struct AppState {
    /// Connection pool for the `SQLite` action-log database.
    pub db: SqlitePool,
}

/// Build the application router with all routes and shared state.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .with_state(state)
}

async fn index() -> &'static str {
    "Eldritch — coming soon"
}

/// Readiness probe: `200 OK` if the database answers a trivial query,
/// `503 Service Unavailable` otherwise.
async fn health(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
