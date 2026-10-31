//! Shared helpers for server integration tests: the mock registries, an in-memory pool, a temporary database file, a
//! spawned server, and a websocket client.
//!
//! Declared once, in the integration root, so a helper no test uses is flagged as dead code.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
use serde::Serialize;
use server::accounts::{self, AccountsError};
use server::auth;
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

/// Serve `app` on an ephemeral loopback port; return its bound address.
pub(crate) async fn serve(app: Router) -> SocketAddr {
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
    serve(server::app(
        AppState::new(pool, REGISTRIES).with_dev_auth(true),
    ))
    .await
}

/// An HTTP client that leaves redirects unfollowed, so a response's
/// `Set-Cookie` is read off the response that set it.
pub(crate) fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("build the HTTP client")
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
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find(|pair| pair.starts_with(&format!("{}=", auth::SESSION_COOKIE)))
        .expect("a session cookie is set")
        .to_string()
}

/// Sign in as the account named `name` through the dev picker at `addr`, the
/// way a browser does; return the session cookie as a `Cookie` header value.
pub(crate) async fn sign_in_over_http(addr: SocketAddr, name: &str) -> String {
    let http = http();
    let page = http
        .get(format!("http://{addr}/auth/dev"))
        .send()
        .await
        .expect("GET the dev picker")
        .text()
        .await
        .expect("the picker page");
    let (id, _) = picker_entries(&page)
        .into_iter()
        .find(|(_, label)| label == name)
        .unwrap_or_else(|| panic!("the picker offers {name}: {page}"));
    let response = http
        .post(format!("http://{addr}/auth/dev"))
        .form(&[("account_id", id)])
        .send()
        .await
        .expect("POST the dev picker");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    session_cookie(response.headers())
}

/// Sign in through the dev picker of `app`, served in-process, as the first
/// account it offers; return the session cookie as a `Cookie` header value.
/// The requests name a loopback `Host`, which the picker requires.
pub(crate) async fn sign_in(app: &Router) -> String {
    let page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/dev")
                .header(header::HOST, "localhost")
                .body(Body::empty())
                .expect("build the picker request"),
        )
        .await
        .expect("GET the dev picker");
    let page = body::to_bytes(page.into_body(), usize::MAX)
        .await
        .expect("the picker page");
    let page = String::from_utf8(page.to_vec()).expect("utf-8 page");
    let (id, _) = picker_entries(&page)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the picker offers an account: {page}"));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/dev")
                .header(header::HOST, "localhost")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("account_id={id}")))
                .expect("build the sign-in request"),
        )
        .await
        .expect("POST the dev picker");
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    session_cookie(response.headers())
}

/// Serve the router for a `POST /games` test: the dev sign-in on and the
/// [`TEST_ACCOUNT`] signed in. Returns the router and the session cookie.
pub(crate) async fn signed_in_app(pool: SqlitePool, registries: Registries) -> (Router, String) {
    add_test_account(&pool).await;
    let app = server::app(AppState::new(pool, registries).with_dev_auth(true));
    let cookie = sign_in(&app).await;
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
