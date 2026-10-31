//! Sign-in sessions: the cookie that names the [`Account`] a request comes
//! from, and the `/auth` routes that report, set and clear it.
//!
//! A session is a row in `sessions` keyed by a random id the cookie carries.
//! It lasts [`SESSION_DAYS`] from the last request it authenticated. An
//! expired session, one whose account is disabled, and a missing or unknown
//! cookie all read as "not signed in", and the account is re-checked on every
//! request, so `server accounts remove` takes effect on the next request or
//! connect.
//!
//! A session starts through one of two sign-ins. Each configured identity
//! provider has its routes in `oauth`, behind the [`provider`] seam. The
//! dev picker is routed when the server runs with `--dev-auth`, and answered
//! only for a loopback `Host`.

pub mod github;
pub(crate) mod oauth;
pub mod provider;

use std::fmt::{Display, Write as _};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{routing, Form, Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use protocol::{AccountId, SignInMethod, SignInRequired, SignedIn};
use serde::Deserialize;
use sqlx::SqlitePool;
use time::Duration;

use crate::accounts::{self, Account};
use crate::auth::oauth::ConfiguredProvider;
use crate::AppState;

/// The name of the cookie that carries the session id.
pub const SESSION_COOKIE: &str = "eldritch_session";

/// The name of the encrypted cookie that holds a provider sign-in between
/// its login and its callback.
pub const SIGN_IN_COOKIE: &str = "eldritch_sign_in";

/// How long a session lasts after the last request it authenticated.
pub const SESSION_DAYS: i64 = 30;

const SESSION_MS: i64 = SESSION_DAYS * 24 * 60 * 60 * 1000;

/// The dev picker's path, as the sign-in screen links it.
const DEV_PICKER: &str = "/auth/dev";

/// The account a request's session signs in as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignedInAccount {
    /// The account's id.
    pub(crate) id: AccountId,
    /// Its host-set display name.
    pub(crate) display_name: String,
    /// The session id, for sliding the cookie's own expiry.
    session_id: SessionId,
}

/// As an extractor, a handler taking a [`SignedInAccount`] answers `401` to a
/// request that isn't signed in. Placed before a body extractor, it answers
/// before the body is read.
impl FromRequestParts<AppState> for SignedInAccount {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        match signed_in(&state.db, &jar).await {
            Ok(Some(account)) => Ok(account),
            Ok(None) => Err(StatusCode::UNAUTHORIZED),
            Err(error) => Err(internal_error("session lookup", &error)),
        }
    }
}

/// The account `jar`'s session cookie signs in as, or `None` when it signs in
/// as nobody. A session this finds live has its expiry moved out to
/// [`SESSION_DAYS`] from now.
///
/// # Errors
///
/// The database failed.
pub(crate) async fn signed_in(
    db: &SqlitePool,
    jar: &CookieJar,
) -> Result<Option<SignedInAccount>, sqlx::Error> {
    let Some(session_id) = SessionId::from_cookie(jar) else {
        return Ok(None);
    };
    let now = now_ms();
    let row: Option<(i64, String)> = sqlx::query_as(
        "UPDATE sessions SET expires_at = ?
         WHERE session_id = ? AND expires_at > ?
           AND account_id IN (SELECT account_id FROM accounts WHERE disabled = 0)
         RETURNING account_id,
           (SELECT display_name FROM accounts WHERE account_id = sessions.account_id)",
    )
    .bind(now + SESSION_MS)
    .bind(session_id.as_str())
    .bind(now)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(id, display_name)| SignedInAccount {
        id: AccountId(id),
        display_name,
        session_id,
    }))
}

/// The `/auth` routes. Any path under `/auth` they don't route answers `404`,
/// rather than falling through to the client bundle as an unrouted path does.
pub(crate) fn router(state: &AppState) -> Router<AppState> {
    let router = Router::new()
        .route("/me", routing::get(me))
        .route("/sign-out", routing::post(sign_out));
    let router = oauth::routes(router, &state.identity_providers);
    let router = if state.dev_auth {
        router.route("/dev", routing::get(dev_picker).post(dev_sign_in))
    } else {
        router
    };
    router.fallback(|| async { StatusCode::NOT_FOUND })
}

/// `GET /auth/me`: `200` with the signed-in account, or `401` listing the
/// sign-in methods this request can use.
async fn me(State(state): State<AppState>, headers: HeaderMap, jar: CookieJar) -> Response {
    match signed_in(&state.db, &jar).await {
        Ok(Some(account)) => (
            refreshed(jar, &account),
            Json(SignedIn {
                account_id: account.id,
                display_name: account.display_name,
            }),
        )
            .into_response(),
        Ok(None) => {
            let mut methods: Vec<SignInMethod> = state
                .identity_providers
                .iter()
                .map(ConfiguredProvider::sign_in_method)
                .collect();
            if dev_picker_answers(&state, &headers) {
                methods.push(SignInMethod {
                    label: "Dev sign-in".to_string(),
                    url: DEV_PICKER.to_string(),
                });
            }
            (StatusCode::UNAUTHORIZED, Json(SignInRequired { methods })).into_response()
        }
        Err(error) => internal_error("session lookup", &error).into_response(),
    }
}

/// `POST /auth/sign-out`: delete the request's session and clear its cookie.
/// Succeeds whether or not there was a session.
async fn sign_out(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(session_id) = SessionId::from_cookie(&jar) {
        if let Err(error) = sqlx::query("DELETE FROM sessions WHERE session_id = ?")
            .bind(session_id.as_str())
            .execute(&state.db)
            .await
        {
            return internal_error("sign-out", &error).into_response();
        }
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    (jar, StatusCode::NO_CONTENT).into_response()
}

/// `GET /auth/dev`: a page listing every enabled account, each a button that
/// signs in as it.
async fn dev_picker(_: LoopbackDevAuth, State(state): State<AppState>) -> Response {
    match accounts::list_enabled(&state.db).await {
        Ok(enabled) => Html(picker_page(&enabled)).into_response(),
        Err(error) => internal_error("listing the dev picker's accounts", &error).into_response(),
    }
}

/// The dev picker's form: the account picked.
#[derive(Deserialize)]
struct DevSignIn {
    account_id: AccountId,
}

/// `POST /auth/dev`: sign in as the picked account, then go to `/`. A picked
/// id that names no enabled account is a `400`.
async fn dev_sign_in(
    _: LoopbackDevAuth,
    State(state): State<AppState>,
    jar: CookieJar,
    Form(form): Form<DevSignIn>,
) -> Response {
    match accounts::is_enabled(&state.db, form.account_id).await {
        Ok(true) => {}
        Ok(false) => return StatusCode::BAD_REQUEST.into_response(),
        Err(error) => return internal_error("account lookup", &error).into_response(),
    }
    match create_session(&state.db, form.account_id).await {
        // Relative, so a sign-in through `trunk serve`'s proxy on :3000
        // lands back on :3000.
        Ok(session_id) => (jar.add(session_cookie(session_id)), Redirect::to("/")).into_response(),
        Err(error) => internal_error("starting a session", &error).into_response(),
    }
}

/// Start a session for `account`; return its id.
async fn create_session(db: &SqlitePool, account: AccountId) -> Result<SessionId, sqlx::Error> {
    let session_id = SessionId::new();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO sessions (session_id, account_id, created_at, expires_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(session_id.as_str())
    .bind(account.0)
    .bind(now)
    .bind(now + SESSION_MS)
    .execute(db)
    .await?;
    Ok(session_id)
}

/// `jar` with the session cookie set again, so the browser's copy expires
/// when the session it carries does.
fn refreshed(jar: CookieJar, account: &SignedInAccount) -> CookieJar {
    jar.add(session_cookie(account.session_id.clone()))
}

/// The session cookie. `Secure` holds on plain-http `localhost` too: MDN,
/// *"The `https:` requirements are ignored when the `Secure` attribute is set
/// by localhost."* `Lax` rather than `Strict`, so the cookie still goes with
/// a top-level navigation from another site, such as a followed link to the
/// game.
fn session_cookie(session_id: SessionId) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, session_id.0))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(Duration::days(SESSION_DAYS))
        .build()
}

/// A session's id, as its cookie carries it and `sessions` keys it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionId(String);

impl SessionId {
    /// A new session id: 256 bits from the OS RNG, hex-encoded.
    fn new() -> Self {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).expect("the OS RNG is available");
        Self(
            bytes
                .iter()
                .fold(String::with_capacity(64), |mut hex, byte| {
                    let _ = write!(hex, "{byte:02x}");
                    hex
                }),
        )
    }

    /// The id `jar`'s session cookie carries, if it has one.
    fn from_cookie(jar: &CookieJar) -> Option<Self> {
        jar.get(SESSION_COOKIE)
            .map(|cookie| Self(cookie.value().to_string()))
    }

    /// The id as stored.
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Proof that the dev picker answers this request (see [`dev_picker_answers`]).
/// As an extractor it answers `404` otherwise, and placed before a body
/// extractor it answers before the body is read, so a public request is `404`
/// whatever it carries.
struct LoopbackDevAuth;

impl FromRequestParts<AppState> for LoopbackDevAuth {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if dev_picker_answers(state, &parts.headers) {
            Ok(Self)
        } else {
            Err(StatusCode::NOT_FOUND)
        }
    }
}

/// Whether the dev picker answers this request: the server runs with
/// `--dev-auth`, and the request's `Host` names this machine. A `Host` from
/// anywhere else is refused even with the flag on, so a flag left on behind
/// the public proxy, which forwards the public `Host`, hands out nothing.
fn dev_picker_answers(state: &AppState, headers: &HeaderMap) -> bool {
    state.dev_auth
        && headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .is_some_and(is_loopback)
}

/// Whether a `Host` header names the loopback interface, whatever its port.
fn is_loopback(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(bracketed) => bracketed
            .split_once(']')
            .map_or(bracketed, |(name, _)| name),
        None => host.split_once(':').map_or(host, |(name, _)| name),
    };
    matches!(name, "localhost" | "127.0.0.1" | "::1")
}

/// The dev picker page offering each of `enabled`.
fn picker_page(enabled: &[Account]) -> String {
    let mut buttons = String::new();
    for account in enabled {
        let _ = write!(
            buttons,
            r#"<p><button name="account_id" value="{}">{}</button></p>"#,
            account.id,
            escape(&account.display_name)
        );
    }
    if buttons.is_empty() {
        buttons.push_str(
            "<p>No account is enabled. Add one with <code>server accounts add</code>.</p>",
        );
    }
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>Dev sign-in</title></head>\
         <body><h1>Dev sign-in</h1><form method=\"post\" action=\"{DEV_PICKER}\">{buttons}</form></body></html>\n"
    )
}

/// `text` with the HTML-significant characters escaped.
fn escape(text: &str) -> String {
    text.chars()
        .fold(String::with_capacity(text.len()), |mut out, c| {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                c => out.push(c),
            }
            out
        })
}

/// Unix time in milliseconds, as the database stores times.
fn now_ms() -> i64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past the epoch");
    i64::try_from(since_epoch.as_millis()).expect("milliseconds fit in i64")
}

/// Log that `operation` failed with `error`, and answer `500`.
pub(crate) fn internal_error(operation: &str, error: &impl Display) -> StatusCode {
    tracing::error!(%error, "{operation} failed");
    StatusCode::INTERNAL_SERVER_ERROR
}
