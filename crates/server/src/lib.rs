//! Eldritch server library: HTTP/WS surface + `SQLite` persistence.
//!
//! The binary (`main.rs`) is a thin wrapper that wires configuration,
//! opens the database, and serves the router built here. Keeping the
//! logic in a library target lets integration tests in `tests/` drive
//! it directly.

pub mod accounts;
pub mod auth;
pub mod config;
pub mod db;
pub mod fingerprint;
mod id;
pub mod lifecycle;
pub mod logged_answer;
pub mod session;
mod store;
mod ws;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::{routing, Router};
use axum_extra::extract::cookie::Key;
use game_core::engine::Registries;
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};
use url::Url;

use crate::auth::oauth::ConfiguredProvider;
use crate::auth::provider::IdentityProvider;
use crate::ws::Rooms;

/// Shared application state handed to every Axum handler.
#[derive(Clone)]
pub struct AppState {
    /// Connection pool for the `SQLite` action-log database.
    pub db: SqlitePool,
    /// The card and scenario registries every game is created, played and
    /// replayed against.
    registries: Registries,
    /// Live games keyed by `game_id`, each with its broadcast group.
    rooms: Rooms,
    /// Directory holding the built client bundle (`index.html`, JS, wasm),
    /// served as the router fallback.
    dist_dir: PathBuf,
    /// Whether the dev sign-in picker is routed (`--dev-auth`).
    dev_auth: bool,
    /// The identity providers offered at sign-in, in the order the sign-in
    /// screen lists them.
    identity_providers: Vec<ConfiguredProvider>,
    /// The key that encrypts the in-flight sign-in cookie. Random per
    /// process, so a restart drops only the sign-ins in flight.
    cookie_key: Key,
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}

impl AppState {
    /// Build application state over a database pool and the registries its
    /// games run against, serving the client bundle from the default dev
    /// location (`crates/web/dist`, relative to the workspace root).
    #[must_use]
    pub fn new(db: SqlitePool, registries: Registries) -> Self {
        Self::new_with_dist(db, registries, PathBuf::from(config::DEV_DIST_DIR))
    }

    /// Build application state with an explicit client-bundle directory.
    #[must_use]
    pub fn new_with_dist(db: SqlitePool, registries: Registries, dist_dir: PathBuf) -> Self {
        Self {
            db,
            registries,
            rooms: ws::rooms(),
            dist_dir,
            dev_auth: false,
            identity_providers: Vec::new(),
            cookie_key: Key::generate(),
        }
    }

    /// Route the dev sign-in picker, `/auth/dev`, or not. Off by default. On,
    /// it answers only requests whose `Host` is loopback.
    #[must_use]
    pub fn with_dev_auth(self, dev_auth: bool) -> Self {
        Self { dev_auth, ..self }
    }

    /// Offer sign-in through each of `providers`, in order, each calling back
    /// to `{public_origin}/auth/{slug}/callback`. None by default.
    #[must_use]
    pub fn with_identity_providers(
        self,
        public_origin: &Url,
        providers: Vec<Arc<dyn IdentityProvider>>,
    ) -> Self {
        let identity_providers = providers
            .into_iter()
            .map(|provider| ConfiguredProvider::new(public_origin, provider))
            .collect();
        Self {
            identity_providers,
            ..self
        }
    }
}

/// The registries the production server creates and plays games against.
///
/// They are the real `scenarios::REGISTRY` (which includes The Gathering) and
/// the real `cards::REGISTRY` (the corpus + hand-written abilities). `main`
/// hands them to [`AppState::new_with_dist`]. Tests pass their own pairing
/// instead and build any mock scenario module locally
/// (`crates/server/tests/integration/common/mod.rs`); there is no shared fixture
/// crate-side (ADR 0016).
pub const REGISTRIES: Registries = Registries {
    cards: &cards::REGISTRY,
    scenarios: &scenarios::REGISTRY,
};

/// Build the application router with all routes and shared state.
///
/// The JSON API, `/auth` and the WebSocket take precedence; everything else
/// falls back to the client bundle, with `index.html` as the SPA fallback.
pub fn app(state: AppState) -> Router {
    let index_html = state.dist_dir.join("index.html");
    // `.fallback()` (not `.not_found_service()`) keeps the response at
    // 200 OK — correct for SPA routing, where the browser loads
    // index.html and its JS resolves the path. `not_found_service`
    // would force a 404, which an SPA shouldn't return for its routes.
    let static_files = ServeDir::new(&state.dist_dir).fallback(ServeFile::new(index_html));

    Router::new()
        .route("/health", routing::get(health))
        .route("/games", routing::post(lifecycle::create_game))
        .nest("/auth", auth::router(&state))
        .route("/ws/{game_id}", routing::get(ws::game_ws))
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
