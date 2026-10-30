//! Eldritch server binary: reads its configuration from the environment,
//! opens the `SQLite` action-log database, applies migrations, and serves the
//! HTTP/WS router.

use std::env;
use std::error::Error;

use server::config::Config;
use server::db::{self, MIGRATOR};
use server::AppState;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Logged before returning: `main`'s error exit prints the `Debug` form,
    // which doesn't name the variable.
    let config = Config::from_lookup(|name| env::var(name).ok())
        .inspect_err(|error| tracing::error!("{error}"))?;

    let pool = db::connect_pool(&config.database_url).await?;
    MIGRATOR.run(&pool).await?;
    tracing::info!("database ready at {}", config.database_url);

    // A missing bundle only warns: the API and WebSocket still serve, which
    // is what a dev run without `trunk build` wants. On a deploy it means a
    // mistyped `ELDRITCH_DIST`, and this line is where the journal shows it.
    let index_html = config.dist_dir.join("index.html");
    if !index_html.is_file() {
        tracing::warn!(
            "no client bundle at {}; serving the API only",
            index_html.display()
        );
    }

    let listener = TcpListener::bind(config.bind).await?;
    tracing::info!(
        "eldritch server (build {}) listening on http://{}",
        protocol::build_id(),
        config.bind
    );

    axum::serve(
        listener,
        server::app(AppState::new_with_dist(
            pool,
            server::REGISTRIES,
            config.dist_dir,
        )),
    )
    .await?;
    Ok(())
}
