//! The server's configuration, read from the environment in one place.
//!
//! With nothing set, every knob takes its dev default, so `cargo run -p
//! server` from the workspace root behaves as it always has. The deploy sets
//! them in the systemd unit.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

/// Where the server listens, what it serves, and which database it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The address the HTTP/WS listener binds (`ELDRITCH_BIND`).
    pub bind: SocketAddr,
    /// The directory holding the built client bundle (`ELDRITCH_DIST`).
    /// A relative path resolves against the working directory.
    pub dist_dir: PathBuf,
    /// The `SQLite` database URL (`DATABASE_URL`).
    pub database_url: String,
}

/// A configuration variable was set to a value the server can't use.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `ELDRITCH_BIND` isn't a socket address (`<ip>:<port>`).
    #[error("{BIND} is not a socket address (<ip>:<port>): {value:?}")]
    InvalidBind {
        /// The value as set.
        value: String,
    },
}

/// The listen address, as `<ip>:<port>`.
const BIND: &str = "ELDRITCH_BIND";
/// The client-bundle directory.
const DIST: &str = "ELDRITCH_DIST";
/// The `SQLite` database URL.
const DATABASE_URL: &str = "DATABASE_URL";

impl Config {
    /// Read the configuration through `lookup`, which returns a variable's
    /// value, or `None` when it is unset. `main` passes the process
    /// environment.
    ///
    /// # Errors
    ///
    /// A set variable whose value doesn't parse.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind = match lookup(BIND) {
            Some(value) => value
                .parse()
                .map_err(|_| ConfigError::InvalidBind { value })?,
            None => SocketAddr::from((Ipv4Addr::LOCALHOST, 8000)),
        };
        Ok(Self {
            bind,
            dist_dir: lookup(DIST).map_or_else(|| PathBuf::from("crates/web/dist"), PathBuf::from),
            database_url: lookup(DATABASE_URL).unwrap_or_else(|| "sqlite:eldritch.db".to_string()),
        })
    }
}
