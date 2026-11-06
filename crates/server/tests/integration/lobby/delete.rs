//! Delete: the creator's `DELETE /games/{id}` removes a game at any stage,
//! closing every socket on it with [`CLOSE_GAME_DELETED`] and taking it off
//! every member's "Your games". Anyone else's `DELETE` is refused and changes
//! nothing.

use std::net::SocketAddr;

use protocol::{ClientMessage, ServerMessage, CLOSE_GAME_DELETED, CLOSE_GAME_NOT_FOUND};
use reqwest::StatusCode;

use super::start::{keep_opening_hand, started_by_tal_and_sam};
use super::{daisy_roland_agnes, recv_lobby, tal_and_sam, TwoMembers};
use crate::common::{self, Client, OTHER_ACCOUNT, TEST_ACCOUNT};
use crate::your_games;

/// `DELETE /games/{game_id}` at `addr`, signed in as the account named
/// `name`, or signed out with `None`.
async fn delete_as(addr: SocketAddr, game_id: &str, name: Option<&str>) -> StatusCode {
    let cookie = match name {
        Some(name) => Some(common::sign_in_over_http(addr, name).await),
        None => None,
    };
    common::send_with_cookie(
        common::http().delete(format!("http://{addr}/games/{game_id}")),
        cookie.as_deref(),
    )
    .await
    .status()
}

/// Whether the account named `name`'s "Your games" lists `game_id`.
async fn listed_for(addr: SocketAddr, game_id: &str, name: &str) -> bool {
    your_games::listing_for(addr, name)
        .await
        .iter()
        .any(|entry| entry.game_id.as_str() == game_id)
}

/// What every successful delete comes to: both sockets close as deleted, the
/// game is on neither member's list, and opening the link again finds no
/// game.
async fn assert_deleted(mut members: TwoMembers) {
    common::assert_closed_with(&mut members.tal_ws, CLOSE_GAME_DELETED).await;
    common::assert_closed_with(&mut members.sam_ws, CLOSE_GAME_DELETED).await;

    assert!(!listed_for(members.addr, &members.game_id, TEST_ACCOUNT).await);
    assert!(!listed_for(members.addr, &members.game_id, OTHER_ACCOUNT).await);

    let mut again = common::connect_as(members.addr, &members.game_id, OTHER_ACCOUNT).await;
    common::assert_closed_with(&mut again, CLOSE_GAME_NOT_FOUND).await;
}

/// `ws` is still open and served: a `Ping` is answered with a `Pong`, and
/// nothing reached it first.
async fn assert_still_open(ws: &mut Client) {
    common::send(ws, &ClientMessage::Ping).await;
    match common::recv_within(ws).await {
        ServerMessage::Pong => {}
        other => panic!("expected a Pong, got {other:?}"),
    }
}

#[tokio::test]
async fn the_creator_deleting_a_lobby_closes_every_socket_and_removes_it_for_everyone() {
    let mut members = tal_and_sam().await;
    daisy_roland_agnes(&mut members).await;

    let status = delete_as(members.addr, &members.game_id, Some(TEST_ACCOUNT)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_deleted(members).await;
}

/// The mulligan answered, so the game has an action, an assignment and
/// members to delete.
#[tokio::test]
async fn the_creator_deleting_a_started_game_closes_every_socket_and_removes_it_for_everyone() {
    let mut members = started_by_tal_and_sam().await;
    common::send(&mut members.sam_ws, &keep_opening_hand(1)).await;
    let _ = common::recv_within(&mut members.sam_ws).await;
    let _ = common::recv_within(&mut members.tal_ws).await;

    let status = delete_as(members.addr, &members.game_id, Some(TEST_ACCOUNT)).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_deleted(members).await;
}

#[tokio::test]
async fn a_members_or_a_signed_out_delete_is_refused_and_deletes_nothing() {
    let mut members = tal_and_sam().await;

    assert_eq!(
        delete_as(members.addr, &members.game_id, Some(OTHER_ACCOUNT)).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        delete_as(members.addr, &members.game_id, None).await,
        StatusCode::UNAUTHORIZED
    );

    assert_still_open(&mut members.tal_ws).await;
    assert_still_open(&mut members.sam_ws).await;
    assert!(listed_for(members.addr, &members.game_id, OTHER_ACCOUNT).await);
    let mut again = common::connect_as(members.addr, &members.game_id, OTHER_ACCOUNT).await;
    let (_, view, _) = recv_lobby(&mut again).await;
    assert_eq!(view.members.len(), 2, "the lobby is as it was");
}

#[tokio::test]
async fn a_started_games_delete_by_a_member_is_refused() {
    let mut members = started_by_tal_and_sam().await;

    assert_eq!(
        delete_as(members.addr, &members.game_id, Some(OTHER_ACCOUNT)).await,
        StatusCode::FORBIDDEN
    );

    assert_still_open(&mut members.sam_ws).await;
    assert!(listed_for(members.addr, &members.game_id, OTHER_ACCOUNT).await);
}

#[tokio::test]
async fn deleting_a_game_that_doesnt_exist_is_not_found() {
    let pool = common::memory_pool().await;
    common::test_account(&pool).await;
    let addr = common::spawn_server(pool).await;

    assert_eq!(
        delete_as(addr, "no-such-game", Some(TEST_ACCOUNT)).await,
        StatusCode::NOT_FOUND
    );
}

/// Listed first, so the registry has cached the game's load failure: the
/// delete must drop it, or the next connection would be told the game is
/// unresumable rather than gone.
#[tokio::test]
async fn an_unresumable_game_is_deleted_and_its_link_then_finds_no_game() {
    let pool = common::memory_pool().await;
    your_games::played_by_an_older_build(&pool, "stale").await;
    common::set_logged(&pool, "stale", 0, "$.response", "Confirm").await;
    let addr = common::spawn_server(pool).await;
    assert!(listed_for(addr, "stale", TEST_ACCOUNT).await);

    assert_eq!(
        delete_as(addr, "stale", Some(TEST_ACCOUNT)).await,
        StatusCode::NO_CONTENT
    );

    assert!(!listed_for(addr, "stale", TEST_ACCOUNT).await);
    let mut again = common::connect(addr, "stale").await;
    common::assert_closed_with(&mut again, CLOSE_GAME_NOT_FOUND).await;
}
