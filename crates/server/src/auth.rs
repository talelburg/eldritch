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
//! The only sign-in here is the dev picker, routed when the server runs with
//! `--dev-auth`, and answered only for a loopback `Host`.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{routing, Form, Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use protocol::{SignInMethod, SignInRequired, SignedIn};
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::accounts::{self, Account, AccountId};
use crate::AppState;

/// The name of the cookie that carries the session id.
pub const SESSION_COOKIE: &str = "eldritch_session";

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
    session_id: String,
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
            Err(error) => {
                tracing::error!(%error, "session lookup failed");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
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
    let Some(cookie) = jar.get(SESSION_COOKIE) else {
        return Ok(None);
    };
    let session_id = cookie.value();
    let now = now_ms();
    let row: Option<(i64, String)> = sqlx::query_as(
        "UPDATE sessions SET expires_at = ?
         WHERE session_id = ? AND expires_at > ?
           AND account_id IN (SELECT account_id FROM accounts WHERE disabled = 0)
         RETURNING account_id,
           (SELECT display_name FROM accounts WHERE account_id = sessions.account_id)",
    )
    .bind(now + SESSION_MS)
    .bind(session_id)
    .bind(now)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(id, display_name)| SignedInAccount {
        id: AccountId(id),
        display_name,
        session_id: session_id.to_string(),
    }))
}

/// The `/auth` routes. Any path under `/auth` they don't route answers `404`,
/// rather than falling through to the client bundle as an unrouted path does.
pub(crate) fn router(dev_auth: bool) -> Router<AppState> {
    let router = Router::new()
        .route("/me", routing::get(me))
        .route("/sign-out", routing::post(sign_out));
    let router = if dev_auth {
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
                account_id: account.id.0,
                display_name: account.display_name,
            }),
        )
            .into_response(),
        Ok(None) => {
            let methods = if dev_picker_answers(&state, &headers) {
                vec![SignInMethod {
                    label: "Dev sign-in".to_string(),
                    url: DEV_PICKER.to_string(),
                }]
            } else {
                Vec::new()
            };
            (StatusCode::UNAUTHORIZED, Json(SignInRequired { methods })).into_response()
        }
        Err(error) => database_error(&error),
    }
}

/// `POST /auth/sign-out`: delete the request's session and clear its cookie.
/// Succeeds whether or not there was a session.
async fn sign_out(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        if let Err(error) = sqlx::query("DELETE FROM sessions WHERE session_id = ?")
            .bind(cookie.value())
            .execute(&state.db)
            .await
        {
            return database_error(&error);
        }
    }
    let jar = jar.remove(Cookie::build(SESSION_COOKIE).path("/"));
    (jar, StatusCode::NO_CONTENT).into_response()
}

/// `GET /auth/dev`: a page listing every enabled account, each a button that
/// signs in as it.
async fn dev_picker(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !dev_picker_answers(&state, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match accounts::list(&state.db).await {
        Ok(entries) => Html(picker_page(&entries)).into_response(),
        Err(error) => {
            tracing::error!(%error, "listing the accounts for the dev picker");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// The dev picker's form: the account picked.
#[derive(Deserialize)]
struct DevSignIn {
    account_id: i64,
}

/// `POST /auth/dev`: sign in as the picked account, then go to `/`. A picked
/// id that names no enabled account is a `400`.
async fn dev_sign_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Form(form): Form<DevSignIn>,
) -> Response {
    if !dev_picker_answers(&state, &headers) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let picked = AccountId(form.account_id);
    let enabled = match accounts::list(&state.db).await {
        Ok(entries) => entries
            .iter()
            .any(|account| account.id == picked && !account.disabled),
        Err(error) => {
            tracing::error!(%error, "listing the accounts for the dev picker");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if !enabled {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match create_session(&state.db, picked).await {
        // Relative, so a sign-in through `trunk serve`'s proxy on :3000
        // lands back on :3000.
        Ok(session_id) => (jar.add(session_cookie(session_id)), Redirect::to("/")).into_response(),
        Err(error) => database_error(&error),
    }
}

/// Start a session for `account`; return its id.
async fn create_session(db: &SqlitePool, account: AccountId) -> Result<String, sqlx::Error> {
    let session_id = new_session_id();
    let now = now_ms();
    sqlx::query(
        "INSERT INTO sessions (session_id, account_id, created_at, expires_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&session_id)
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
fn session_cookie(session_id: String) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, session_id))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::days(SESSION_DAYS))
        .build()
}

/// A new session id: 256 bits from the OS RNG, hex-encoded.
fn new_session_id() -> String {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).expect("the OS RNG is available");
    bytes
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
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

/// The dev picker page for `entries`, listing the enabled ones.
fn picker_page(entries: &[Account]) -> String {
    let mut buttons = String::new();
    for account in entries.iter().filter(|account| !account.disabled) {
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

/// Log a database failure and answer `500`.
fn database_error(error: &sqlx::Error) -> Response {
    tracing::error!(%error, "session lookup failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
