//! Shared helpers for server integration tests: the mock registries, an in-memory pool, a temporary database file, a
//! spawned server, and a websocket client.
//!
//! Declared once, in the integration root, so a helper no test uses is flagged as dead code.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use axum::body::{self, Body};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use futures_util::{SinkExt as _, StreamExt as _};
use game_core::action::RosterEntry;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::{ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry};
use game_core::state::{CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder};
use game_core::test_support;
use protocol::{ClientMessage, ServerMessage};
use reqwest::redirect::Policy;
use reqwest::{Client as HttpClient, RequestBuilder, Response};
use serde::Serialize;
use server::accounts::{self, AccountsError};
use server::auth;
use server::auth::provider::{
    ExchangeError, ExchangeFuture, IdentityProvider, PendingSignIn, ReportedEmail,
};
use server::db::MIGRATOR;
use server::session::GameSession;
use server::AppState;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tower::ServiceExt as _;
use url::Url;

pub(crate) const TEST_SCENARIO_ID: &str = "test-scenario";

fn test_setup() -> GameState {
    // Bare scenario seed — `seat_and_open` seats the investigators at creation,
    // so neither the pre-seeded investigator nor the seating is injected here.
    GameStateBuilder::new()
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_scenario_id(ScenarioId::new(TEST_SCENARIO_ID))
        .with_rng_seed(42)
        .build()
}

fn noop_resolution(_: ScenarioEnding, _: &mut GameState, _: &mut Vec<Event>) {}

static TEST_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup: test_setup,
    apply_resolution: noop_resolution,
    layout: &[],
};

fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == TEST_SCENARIO_ID).then_some(&TEST_MODULE)
}

/// The mock scenario registry + game-core's synthetic card registry: what
/// every test that creates a game hands the server.
pub(crate) const REGISTRIES: Registries = Registries {
    cards: &test_support::TEST_REGISTRY,
    scenarios: &ScenarioRegistry { module_for },
};

/// A one-investigator roster using the synthetic `TEST_INV` code.
pub(crate) fn roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: vec![],
        random_basic_weaknesses: 0,
    }]
}

/// Create a game of the mock scenario with [`roster`], led by its one
/// investigator, and return the live session at the mulligan prompt.
pub(crate) async fn create_game(pool: &SqlitePool, game_id: &str) -> GameSession {
    GameSession::create(
        pool.clone(),
        REGISTRIES,
        game_id,
        ScenarioId::new(TEST_SCENARIO_ID),
        roster(),
        0,
    )
    .await
    .expect("create game")
}

/// A migrated single-connection in-memory pool.
pub(crate) async fn memory_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

/// A uniquely named `SQLite` file in the temp directory, for a test that needs
/// a database a second process can open. The file isn't created here, and it
/// is removed on drop with its WAL and shared-memory files.
pub(crate) struct TempDb {
    path: PathBuf,
}

impl TempDb {
    /// A fresh path whose name starts `eldritch-<label>-`. The counter keeps
    /// two tests started in the same nanosecond apart.
    pub(crate) fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after the epoch")
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            path: env::temp_dir().join(format!("eldritch-{label}-{nanos}-{n}.db")),
        }
    }

    /// The file's path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The `sqlite:` URL that opens the file.
    pub(crate) fn url(&self) -> String {
        format!("sqlite:{}", self.path.display())
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.path.clone().into_os_string();
            path.push(suffix);
            let _ = fs::remove_file(path);
        }
    }
}

/// A committed client bundle, for a test whose request would otherwise fall
/// through to a missing one: the real `crates/web/dist/` is Trunk's output,
/// absent in CI and fresh checkouts.
pub(crate) const FIXTURE_DIST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dist");

/// The display name of the account [`spawn_server`] adds, which [`connect`]
/// signs in as.
pub(crate) const TEST_ACCOUNT: &str = "Tester";

/// Add the [`TEST_ACCOUNT`] entry to `pool` unless it is there already, so a
/// test that serves the same database twice can call it twice.
pub(crate) async fn add_test_account(pool: &SqlitePool) {
    let email = "tester@example.com".to_string();
    match accounts::add(pool, TEST_ACCOUNT, &[email]).await {
        Ok(_) | Err(AccountsError::EmailTaken { .. }) => {}
        Err(error) => panic!("add the test account: {error}"),
    }
}

/// The server's router over `pool` and `registries`, with the dev sign-in
/// `dev_auth` on or off.
pub(crate) fn dev_auth_app(pool: SqlitePool, registries: Registries, dev_auth: bool) -> Router {
    server::app(AppState::new(pool, registries).with_dev_auth(dev_auth))
}

/// Serve [`dev_auth_app`] over `pool` and the mock [`REGISTRIES`] on an
/// ephemeral loopback port; return its bound address.
pub(crate) async fn serve_dev_auth(pool: SqlitePool, dev_auth: bool) -> SocketAddr {
    let app = dev_auth_app(pool, REGISTRIES, dev_auth);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    addr
}

/// Spawn the server with the dev sign-in on and the [`TEST_ACCOUNT`] on its
/// allowlist; return its bound address.
pub(crate) async fn spawn_server(pool: SqlitePool) -> SocketAddr {
    add_test_account(&pool).await;
    serve_dev_auth(pool, true).await
}

/// An HTTP client that leaves redirects unfollowed, so a response's
/// `Set-Cookie` is read off the response that set it.
pub(crate) fn http() -> HttpClient {
    HttpClient::builder()
        .redirect(Policy::none())
        .build()
        .expect("build the HTTP client")
}

/// Send `request`, with `cookie` as its `Cookie` header if there is one.
pub(crate) async fn send_with_cookie(request: RequestBuilder, cookie: Option<&str>) -> Response {
    let request = match cookie {
        Some(cookie) => request.header(header::COOKIE, cookie),
        None => request,
    };
    request.send().await.expect("the server answers")
}

/// `GET /auth/me` at `addr`, with `cookie` if there is one.
pub(crate) async fn me(addr: SocketAddr, cookie: Option<&str>) -> Response {
    send_with_cookie(http().get(format!("http://{addr}/auth/me")), cookie).await
}

/// Each account the dev picker page offers: its id and its name.
pub(crate) fn picker_entries(page: &str) -> Vec<(String, String)> {
    const BUTTON: &str = r#"<button name="account_id" value=""#;
    page.split(BUTTON)
        .skip(1)
        .map(|rest| {
            let (id, rest) = rest.split_once("\">").expect("a button's value");
            let (name, _) = rest.split_once("</button>").expect("a button's label");
            (id.to_string(), name.to_string())
        })
        .collect()
}

/// The `name=value` pair of the session cookie a response sets.
pub(crate) fn session_cookie(headers: &HeaderMap) -> String {
    set_cookie(headers, auth::SESSION_COOKIE)
        .expect("a session cookie is set")
        .split(';')
        .next()
        .expect("a name=value pair")
        .to_string()
}

/// The whole `Set-Cookie` value a response sets for the cookie `name`,
/// attributes included, if it sets one.
pub(crate) fn set_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with(&format!("{name}=")))
        .map(ToString::to_string)
}

/// How many rows `sessions` holds.
pub(crate) async fn session_count(pool: &SqlitePool) -> i64 {
    let (sessions,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
        .fetch_one(pool)
        .await
        .expect("count sessions");
    sessions
}

/// The public origin a [`FakeProvider`] app is configured with.
pub(crate) const FAKE_ORIGIN: &str = "http://localhost:3000";

/// The `state` every [`FakeProvider`] sign-in carries.
pub(crate) const FAKE_STATE: &str = "fake-state";

/// The PKCE verifier a [`FakeProvider`] sign-in carries, which its exchange
/// requires back, so a test passes only if the in-flight cookie returned it.
const FAKE_VERIFIER: &str = "fake-verifier";

/// The code a [`FakeProvider`]'s exchange fails on, as a provider that
/// answered with an error would.
pub(crate) const FAILING_CODE: &str = "fails-at-the-provider";

/// An identity provider, `fake` and named "Fake", that reports `emails` for
/// any code but [`FAILING_CODE`].
pub(crate) struct FakeProvider {
    emails: Vec<ReportedEmail>,
}

impl FakeProvider {
    /// A provider reporting each `(address, verified)` of `emails`.
    pub(crate) fn reporting(emails: &[(&str, bool)]) -> Self {
        Self {
            emails: emails
                .iter()
                .map(|&(address, verified)| ReportedEmail {
                    address: address.to_string(),
                    verified,
                })
                .collect(),
        }
    }
}

impl IdentityProvider for FakeProvider {
    fn slug(&self) -> &'static str {
        "fake"
    }

    fn name(&self) -> &'static str {
        "Fake"
    }

    fn authorize_url(&self, redirect_uri: &Url) -> (Url, PendingSignIn) {
        let mut url = Url::parse("https://fake.example/authorize").expect("a URL");
        url.query_pairs_mut()
            .append_pair("redirect_uri", redirect_uri.as_str())
            .append_pair("state", FAKE_STATE);
        (
            url,
            PendingSignIn::new(FAKE_STATE.to_string(), FAKE_VERIFIER.to_string()),
        )
    }

    fn exchange<'a>(
        &'a self,
        code: String,
        pending: PendingSignIn,
        redirect_uri: &'a Url,
    ) -> ExchangeFuture<'a> {
        Box::pin(async move {
            assert_eq!(pending.pkce_verifier(), FAKE_VERIFIER);
            assert_eq!(
                redirect_uri.as_str(),
                format!("{FAKE_ORIGIN}/auth/fake/callback")
            );
            if code == FAILING_CODE {
                Err(ExchangeError::new("the provider refused the code"))
            } else {
                Ok(self.emails.clone())
            }
        })
    }
}

/// The server's router over `pool` with `provider` on, at [`FAKE_ORIGIN`],
/// and the dev sign-in `dev_auth` on or off.
pub(crate) fn provider_app(pool: SqlitePool, provider: FakeProvider, dev_auth: bool) -> Router {
    let origin = Url::parse(FAKE_ORIGIN).expect("a URL");
    server::app(
        AppState::new(pool, REGISTRIES)
            .with_dev_auth(dev_auth)
            .with_identity_providers(&origin, vec![Arc::new(provider)]),
    )
}

/// `GET uri` on `app` with a loopback `Host` and `cookie` if there is one:
/// the response's status, headers and body.
pub(crate) async fn get(
    app: &Router,
    uri: &str,
    cookie: Option<&str>,
) -> (StatusCode, HeaderMap, String) {
    let mut request = Request::builder()
        .uri(uri)
        .header(header::HOST, "localhost");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).expect("build the request"))
        .await
        .expect("a response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        headers,
        String::from_utf8(body.to_vec()).expect("utf-8 body"),
    )
}

/// The two requests a dev-picker sign-in makes, over whichever transport.
trait DevPicker {
    /// `GET /auth/dev`: the picker page.
    async fn page(&self) -> String;
    /// `POST /auth/dev` picking `id`: the response's status and headers.
    async fn pick(&self, id: &str) -> (StatusCode, HeaderMap);
}

/// The dev picker of a server listening at an address, reached over HTTP.
struct OverHttp(SocketAddr);

impl DevPicker for OverHttp {
    async fn page(&self) -> String {
        http()
            .get(format!("http://{}/auth/dev", self.0))
            .send()
            .await
            .expect("GET the dev picker")
            .text()
            .await
            .expect("the picker page")
    }

    async fn pick(&self, id: &str) -> (StatusCode, HeaderMap) {
        let response = http()
            .post(format!("http://{}/auth/dev", self.0))
            .form(&[("account_id", id)])
            .send()
            .await
            .expect("POST the dev picker");
        (response.status(), response.headers().clone())
    }
}

/// The dev picker of a router served in-process. Its requests name a
/// loopback `Host`, which the picker requires.
struct InProcess<'a>(&'a Router);

impl DevPicker for InProcess<'_> {
    async fn page(&self) -> String {
        let request = Request::builder()
            .uri("/auth/dev")
            .header(header::HOST, "localhost")
            .body(Body::empty())
            .expect("build the picker request");
        let response = self
            .0
            .clone()
            .oneshot(request)
            .await
            .expect("GET the dev picker");
        let page = body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the picker page");
        String::from_utf8(page.to_vec()).expect("utf-8 page")
    }

    async fn pick(&self, id: &str) -> (StatusCode, HeaderMap) {
        let request = Request::builder()
            .method("POST")
            .uri("/auth/dev")
            .header(header::HOST, "localhost")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(format!("account_id={id}")))
            .expect("build the sign-in request");
        let response = self
            .0
            .clone()
            .oneshot(request)
            .await
            .expect("POST the dev picker");
        (response.status(), response.headers().clone())
    }
}

/// Sign in as the account named `name` through `picker`, the way a browser
/// does; return the session cookie as a `Cookie` header value.
async fn sign_in_through(picker: impl DevPicker, name: &str) -> String {
    let page = picker.page().await;
    let (id, _) = picker_entries(&page)
        .into_iter()
        .find(|(_, label)| label == name)
        .unwrap_or_else(|| panic!("the picker offers {name}: {page}"));
    let (status, headers) = picker.pick(&id).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    session_cookie(&headers)
}

/// Sign in as the account named `name` through the dev picker at `addr`;
/// return the session cookie as a `Cookie` header value.
pub(crate) async fn sign_in_over_http(addr: SocketAddr, name: &str) -> String {
    sign_in_through(OverHttp(addr), name).await
}

/// Serve the router for a `POST /games` test: the dev sign-in on and the
/// [`TEST_ACCOUNT`] signed in. Returns the router and the session cookie.
pub(crate) async fn signed_in_app(pool: SqlitePool, registries: Registries) -> (Router, String) {
    add_test_account(&pool).await;
    let app = dev_auth_app(pool, registries, true);
    let cookie = sign_in_through(InProcess(&app), TEST_ACCOUNT).await;
    (app, cookie)
}

pub(crate) type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Sign in as the [`TEST_ACCOUNT`] through the dev picker, then open a
/// websocket to a game with that session.
pub(crate) async fn connect(addr: SocketAddr, game_id: &str) -> Client {
    let cookie = sign_in_over_http(addr, TEST_ACCOUNT).await;
    connect_with(addr, game_id, Some(&cookie)).await
}

/// Open a websocket to a game, sending `cookie` on the upgrade if there is one.
pub(crate) async fn connect_with(addr: SocketAddr, game_id: &str, cookie: Option<&str>) -> Client {
    let mut request = format!("ws://{addr}/ws/{game_id}")
        .into_client_request()
        .expect("a websocket request");
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert(header::COOKIE, cookie.parse().expect("a cookie header"));
    }
    let (ws, _response) = connect_async(request).await.expect("ws connect");
    ws
}

/// Read `ws` to its end and require it to be closed with
/// [`protocol::CLOSE_UNAUTHENTICATED`] before any message arrives.
pub(crate) async fn assert_closed_unauthenticated(ws: &mut Client) {
    loop {
        match ws
            .next()
            .await
            .expect("a close frame")
            .expect("no ws error")
        {
            Message::Close(Some(frame)) => {
                assert_eq!(
                    frame.code,
                    CloseCode::from(protocol::CLOSE_UNAUTHENTICATED),
                    "closed as unauthenticated"
                );
                return;
            }
            Message::Text(text) => panic!("no message before the close: {text}"),
            Message::Close(None) => panic!("the close carries a code"),
            _ => {}
        }
    }
}

/// Send a client message as JSON text.
pub(crate) async fn send(ws: &mut Client, msg: &ClientMessage) {
    let json = serde_json::to_string(msg).expect("serialize ClientMessage");
    ws.send(Message::Text(json.into())).await.expect("ws send");
}

/// Receive the next [`ServerMessage`], skipping ping/pong frames.
pub(crate) async fn recv(ws: &mut Client) -> ServerMessage {
    loop {
        match ws.next().await.expect("stream open").expect("no ws error") {
            Message::Text(text) => {
                return serde_json::from_str(text.as_str()).expect("valid ServerMessage");
            }
            Message::Close(_) => panic!("server closed the connection unexpectedly"),
            _ => {}
        }
    }
}

/// Set one JSON field, at `path`, of the answer logged at `seq` for `game_id`:
/// the stored-row edit that simulates an engine change between `apply` and
/// `load`.
pub(crate) async fn set_logged(
    pool: &SqlitePool,
    game_id: &str,
    seq: i64,
    path: &str,
    value: impl Serialize,
) {
    let value = serde_json::to_string(&value).expect("serialize the edited value");
    let edited = sqlx::query(
        "UPDATE actions SET answer = json_set(answer, ?, json(?)) WHERE game_id = ? AND seq = ?",
    )
    .bind(path)
    .bind(value)
    .bind(game_id)
    .bind(seq)
    .execute(pool)
    .await
    .expect("edit the logged answer");
    assert_eq!(edited.rows_affected(), 1, "one logged answer is edited");
}

/// How many `games` and `actions` rows `game_id` has.
pub(crate) async fn stored_rows(pool: &SqlitePool, game_id: &str) -> (i64, i64) {
    let (games,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM games WHERE game_id = ?")
        .bind(game_id)
        .fetch_one(pool)
        .await
        .expect("count games");
    let (actions,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM actions WHERE game_id = ?")
        .bind(game_id)
        .fetch_one(pool)
        .await
        .expect("count actions");
    (games, actions)
}
