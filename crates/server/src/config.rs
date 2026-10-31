//! The server's configuration, read from the environment in one place.
//!
//! With nothing set, every knob takes its dev default and no sign-in
//! provider is on, so `cargo run -p server` from the workspace root serves
//! with the dev sign-in as its only way in.

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use url::Url;

/// Where the server listens, what it serves, which database it opens, and
/// which sign-in providers it offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The address the HTTP/WS listener binds (`ELDRITCH_BIND`).
    pub bind: SocketAddr,
    /// The directory holding the built client bundle (`ELDRITCH_DIST`).
    /// A relative path resolves against the working directory.
    pub dist_dir: PathBuf,
    /// The `SQLite` database URL (`DATABASE_URL`).
    pub database_url: String,
    /// The origin a browser reaches the server at (`ELDRITCH_PUBLIC_ORIGIN`),
    /// such as `https://eldritch.gripe`, or `http://localhost:3000` through
    /// `trunk serve`. A provider's redirect URI is built from it, never from
    /// a request's `Host`, which the trunk proxy rewrites.
    pub public_origin: Option<Url>,
    /// The GitHub OAuth app (`ELDRITCH_GITHUB_CLIENT_ID` and
    /// `ELDRITCH_GITHUB_CLIENT_SECRET`), when GitHub sign-in is on.
    pub github: Option<ProviderCredentials>,
}

/// The client id and secret a provider issued for this server's app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCredentials {
    /// The app's client id. It isn't secret: the provider shows it in every
    /// authorization URL.
    pub client_id: String,
    /// The app's client secret.
    pub client_secret: Secret,
}

/// A value that must stay out of logs: its `Debug` form is `Secret(..)`, and
/// it has no `Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Hold `value` as a secret.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// The secret itself, for handing to the provider.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// A configuration variable was set to a value the server can't use, or a
/// variable another one needs is missing. No variant carries a secret's
/// value.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// `ELDRITCH_BIND` isn't a socket address (`<ip>:<port>`).
    #[error("{BIND} is not a socket address (<ip>:<port>): {value:?}")]
    InvalidBind {
        /// The value as set.
        value: String,
    },
    /// `ELDRITCH_PUBLIC_ORIGIN` isn't an `http` or `https` origin with no
    /// path, query or credentials.
    #[error("{PUBLIC_ORIGIN} is not an http(s) origin such as https://eldritch.gripe: {value:?}")]
    InvalidPublicOrigin {
        /// The value as set.
        value: String,
    },
    /// One of a provider's two variables is set and the other isn't.
    #[error("{set} is set but {missing} is not: a provider needs both")]
    HalfSetProvider {
        /// The variable that is set.
        set: &'static str,
        /// The variable that isn't.
        missing: &'static str,
    },
    /// A provider's variable is set to the empty string.
    #[error("{name} is set but empty")]
    EmptyProviderVariable {
        /// The variable.
        name: &'static str,
    },
    /// A provider is on but `ELDRITCH_PUBLIC_ORIGIN`, which its redirect URI
    /// is built from, isn't set.
    #[error(
        "{provider} is set but {PUBLIC_ORIGIN} is not: a provider's callback is built from it"
    )]
    MissingPublicOrigin {
        /// The provider's client-id variable.
        provider: &'static str,
    },
}

/// The listen address, as `<ip>:<port>`.
const BIND: &str = "ELDRITCH_BIND";
/// The client-bundle directory.
const DIST: &str = "ELDRITCH_DIST";
/// The `SQLite` database URL.
const DATABASE_URL: &str = "DATABASE_URL";
/// The origin a browser reaches the server at.
const PUBLIC_ORIGIN: &str = "ELDRITCH_PUBLIC_ORIGIN";
/// The GitHub OAuth app's client id.
const GITHUB_CLIENT_ID: &str = "ELDRITCH_GITHUB_CLIENT_ID";
/// The GitHub OAuth app's client secret.
const GITHUB_CLIENT_SECRET: &str = "ELDRITCH_GITHUB_CLIENT_SECRET";

/// Where `trunk build` writes the client bundle, relative to the workspace
/// root: the bundle directory when `ELDRITCH_DIST` is unset.
pub(crate) const DEV_DIST_DIR: &str = "crates/web/dist";

impl Config {
    /// Read the configuration through `lookup`, which returns a variable's
    /// value, or `None` when it is unset. `main` passes the process
    /// environment. A provider is on when both its client id and its secret
    /// are set.
    ///
    /// # Errors
    ///
    /// - [`ConfigError::InvalidBind`] when `ELDRITCH_BIND` is set to anything
    ///   that doesn't parse as a socket address, the empty string included.
    /// - [`ConfigError::InvalidPublicOrigin`] when `ELDRITCH_PUBLIC_ORIGIN`
    ///   is set to anything but an `http(s)` origin, the empty string
    ///   included. One trailing `/` is allowed.
    /// - [`ConfigError::HalfSetProvider`] or
    ///   [`ConfigError::EmptyProviderVariable`] when a provider's id or
    ///   secret is set without the other, or set empty.
    /// - [`ConfigError::MissingPublicOrigin`] when a provider is on and
    ///   `ELDRITCH_PUBLIC_ORIGIN` is unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let bind = match lookup(BIND) {
            Some(value) => value
                .parse()
                .map_err(|_| ConfigError::InvalidBind { value })?,
            None => SocketAddr::from((Ipv4Addr::LOCALHOST, 8000)),
        };
        let public_origin = lookup(PUBLIC_ORIGIN).map(public_origin).transpose()?;
        let github = provider(&lookup, GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET)?;
        if github.is_some() && public_origin.is_none() {
            return Err(ConfigError::MissingPublicOrigin {
                provider: GITHUB_CLIENT_ID,
            });
        }
        Ok(Self {
            bind,
            dist_dir: lookup(DIST).map_or_else(|| PathBuf::from(DEV_DIST_DIR), PathBuf::from),
            database_url: lookup(DATABASE_URL).unwrap_or_else(|| "sqlite:eldritch.db".to_string()),
            public_origin,
            github,
        })
    }
}

/// `value` read as the public origin.
fn public_origin(value: String) -> Result<Url, ConfigError> {
    match Url::parse(&value) {
        Ok(url)
            if matches!(url.scheme(), "http" | "https")
                && url.has_host()
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none() =>
        {
            Ok(url)
        }
        _ => Err(ConfigError::InvalidPublicOrigin { value }),
    }
}

/// The provider whose client id is `id_var` and secret `secret_var`: `None`
/// when neither is set.
fn provider(
    lookup: impl Fn(&str) -> Option<String>,
    id_var: &'static str,
    secret_var: &'static str,
) -> Result<Option<ProviderCredentials>, ConfigError> {
    let non_empty = |name: &'static str| match lookup(name) {
        Some(value) if value.is_empty() => Err(ConfigError::EmptyProviderVariable { name }),
        value => Ok(value),
    };
    match (non_empty(id_var)?, non_empty(secret_var)?) {
        (None, None) => Ok(None),
        (Some(client_id), Some(secret)) => Ok(Some(ProviderCredentials {
            client_id,
            client_secret: Secret::new(secret),
        })),
        (Some(_), None) => Err(ConfigError::HalfSetProvider {
            set: id_var,
            missing: secret_var,
        }),
        (None, Some(_)) => Err(ConfigError::HalfSetProvider {
            set: secret_var,
            missing: id_var,
        }),
    }
}
