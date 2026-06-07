//! `SQLite` persistence: connection pool + schema migrations.

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

/// Open a connection pool to `database_url`, creating the database file
/// if it does not yet exist.
///
/// `database_url` is a standard `SQLite` URL, e.g. `sqlite:eldritch.db` or
/// `sqlite::memory:`. Migrations are applied separately via
/// [`MIGRATOR`]; this only establishes the pool.
pub async fn connect_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    SqlitePoolOptions::new().connect_with(options).await
}

/// Embedded SQL migrations, applied in order at startup.
///
/// The migration files live in `crates/server/migrations/` and are
/// baked into the binary at compile time, so a deployed server needs no
/// external migration files to bring a fresh database up to schema.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
