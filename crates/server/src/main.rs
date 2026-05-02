//! Eldritch server binary. Phase 0 placeholder — single GET / endpoint.

use axum::{routing::get, Router};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let app = Router::new().route("/", get(index));

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    tracing::info!("eldritch server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> &'static str {
    "Eldritch — coming soon"
}
