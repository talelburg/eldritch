//! Eldritch server binary: opens the `SQLite` action-log database, applies
//! migrations, and serves the HTTP/WS router.

use std::net::SocketAddr;

use server::{app, db, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:eldritch.db".to_string());
    let pool = db::connect_pool(&database_url).await?;
    db::MIGRATOR.run(&pool).await?;
    tracing::info!("database ready at {database_url}");

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("eldritch server listening on http://{addr}");

    axum::serve(listener, app(AppState { db: pool })).await?;
    Ok(())
}
