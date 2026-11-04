//! The lobby: `POST /games` without a roster stores a lobby, and every
//! signed-in account that opens its link becomes a member. Two accounts over
//! real HTTP and WebSocket, as the spec's server seam describes.
//!
//! The shared helpers live here; each lobby command's tests get a file of
//! their own.

mod picks;
mod start;

use std::net::SocketAddr;

use game_core::action::{InputResponse, PlayerAction};
use game_core::state::investigator::InvestigatorId;
use protocol::lobby::LobbyView;
use protocol::{AccountId, ClientMessage, GameId, LogPosition, Refusal, ServerMessage};
use serde_json::Value;

use crate::common::{self, Client, OTHER_ACCOUNT, TEST_ACCOUNT, TEST_SCENARIO_ID};

/// Create a lobby of the mock scenario over HTTP, signed in as the account
/// named `name`; return its game id.
pub(crate) async fn create_lobby(addr: SocketAddr, name: &str) -> String {
    create_lobby_of(addr, name, TEST_SCENARIO_ID).await
}

/// Create a lobby of `scenario_id` over HTTP, signed in as the account named
/// `name`; return its game id.
pub(crate) async fn create_lobby_of(addr: SocketAddr, name: &str, scenario_id: &str) -> String {
    let cookie = common::sign_in_over_http(addr, name).await;
    let response = common::send_with_cookie(
        common::http()
            .post(format!("http://{addr}/games"))
            .header("content-type", "application/json")
            .body(format!(r#"{{"scenario_id":"{scenario_id}"}}"#)),
        Some(&cookie),
    )
    .await;
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let body = response.text().await.expect("a body");
    let json: Value = serde_json::from_str(&body).expect("a JSON body");
    json["game_id"]
        .as_str()
        .expect("game_id is a string")
        .to_string()
}

/// The next frame on `ws`, which must be a `Lobby`: its `you`, view and
/// newly-joined flag.
pub(crate) async fn recv_lobby(ws: &mut Client) -> (AccountId, LobbyView, bool) {
    match common::recv_within(ws).await {
        ServerMessage::Lobby {
            build,
            you,
            view,
            joined,
        } => {
            assert_eq!(build, protocol::build_id(), "the Lobby names the build");
            (you, view, joined)
        }
        other => panic!("expected a Lobby, got {other:?}"),
    }
}

/// The members a view lists, by display name, in the order it lists them.
pub(crate) fn member_names(view: &LobbyView) -> Vec<&str> {
    view.members
        .iter()
        .map(|member| member.display_name.as_str())
        .collect()
}

#[tokio::test]
async fn opening_the_link_makes_the_second_account_a_member_and_both_see_it() {
    let pool = common::memory_pool().await;
    let tester = common::test_account(&pool).await;
    let other = common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool).await;
    let game_id = create_lobby(addr, TEST_ACCOUNT).await;

    let mut tester_ws = common::connect(addr, &game_id).await;
    let (you, view, joined) = recv_lobby(&mut tester_ws).await;
    assert_eq!(you, tester);
    assert_eq!(view.creator, tester, "the account that created it");
    assert_eq!(
        member_names(&view),
        [TEST_ACCOUNT],
        "the creator is a member"
    );
    assert!(!joined, "creating the game made the creator a member");
    assert!(view.picks.is_empty() && view.lead.is_none());

    let mut other_ws = common::connect_as(addr, &game_id, OTHER_ACCOUNT).await;
    let (you, view, joined) = recv_lobby(&mut other_ws).await;
    assert_eq!(you, other);
    assert_eq!(member_names(&view), [TEST_ACCOUNT, OTHER_ACCOUNT]);
    assert!(joined, "opening the link just made this account a member");

    let (you, view, joined) = recv_lobby(&mut tester_ws).await;
    assert_eq!(you, tester, "each connection is sent its own Lobby");
    assert_eq!(member_names(&view), [TEST_ACCOUNT, OTHER_ACCOUNT]);
    assert!(!joined, "the join is someone else's");
}

#[tokio::test]
async fn opening_the_link_again_is_not_a_new_join() {
    let pool = common::memory_pool().await;
    common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool).await;
    let game_id = create_lobby(addr, TEST_ACCOUNT).await;

    let mut first = common::connect_as(addr, &game_id, OTHER_ACCOUNT).await;
    let (_, _, joined) = recv_lobby(&mut first).await;
    assert!(joined);
    drop(first);

    let mut second = common::connect_as(addr, &game_id, OTHER_ACCOUNT).await;
    let (_, view, joined) = recv_lobby(&mut second).await;
    assert!(!joined, "the account was already a member");
    assert_eq!(member_names(&view), [TEST_ACCOUNT, OTHER_ACCOUNT]);
}

#[tokio::test]
async fn opening_the_link_of_a_started_game_makes_the_account_a_member() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-started").await;
    let other = common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool.clone()).await;

    let mut other_ws = common::connect_as(addr, "g-started", OTHER_ACCOUNT).await;
    assert!(
        matches!(
            common::recv_within(&mut other_ws).await,
            ServerMessage::Hello { .. }
        ),
        "a started game opens on its board"
    );

    let members = server::lobby::members_of(&pool, &GameId::new("g-started"))
        .await
        .expect("read the members");
    assert!(members.contains(&other), "{members:?}");
}

/// A lobby has no prompt, so a client answering one has a bug.
#[tokio::test]
async fn an_answer_sent_to_a_lobby_is_refused_as_not_started() {
    let pool = common::memory_pool().await;
    let addr = common::spawn_server(pool).await;
    let game_id = create_lobby(addr, TEST_ACCOUNT).await;
    let mut ws = common::connect(addr, &game_id).await;
    let _ = recv_lobby(&mut ws).await;

    common::send(
        &mut ws,
        &ClientMessage::Submit {
            prompt: LogPosition(0),
            action: PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::Confirm,
            },
        },
    )
    .await;

    match common::recv_within(&mut ws).await {
        ServerMessage::Rejected(Refusal::NotStarted) => {}
        other => panic!("expected a NotStarted refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_restart_mid_lobby_keeps_the_lobby_and_its_members() {
    let pool = common::memory_pool().await;
    common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool.clone()).await;
    let game_id = create_lobby(addr, TEST_ACCOUNT).await;
    let mut other_ws = common::connect_as(addr, &game_id, OTHER_ACCOUNT).await;
    let _ = recv_lobby(&mut other_ws).await;

    // A fresh server with empty rooms over the same database.
    let restarted = common::spawn_server(pool).await;
    let mut tester_ws = common::connect(restarted, &game_id).await;
    let (_, view, _) = recv_lobby(&mut tester_ws).await;
    assert_eq!(member_names(&view), [TEST_ACCOUNT, OTHER_ACCOUNT]);
    assert_eq!(view.scenario_id.as_str(), TEST_SCENARIO_ID);
}
