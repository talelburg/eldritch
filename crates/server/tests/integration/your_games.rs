//! `GET /games`: the "Your games" listing of every game the signed-in account
//! is a member of, over real HTTP with two accounts.

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use protocol::{AccountId, GameEntry, GameEntryState, GameId};
use reqwest::StatusCode;

use crate::common::{self, OTHER_ACCOUNT, TEST_ACCOUNT, TEST_SCENARIO_ID};
use crate::lobby::{self, DAISY, ROLAND};

/// `GET /games` at `addr` with `cookie`, if there is one.
async fn get_games(addr: SocketAddr, cookie: Option<&str>) -> reqwest::Response {
    common::send_with_cookie(common::http().get(format!("http://{addr}/games")), cookie).await
}

/// The listing `GET /games` returns to the account named `name`.
async fn listing_for(addr: SocketAddr, name: &str) -> Vec<GameEntry> {
    let cookie = common::sign_in_over_http(addr, name).await;
    let response = get_games(addr, Some(&cookie)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("a body");
    serde_json::from_str(&body).expect("a listing")
}

/// Now, in unix millis.
fn now_millis() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch");
    i64::try_from(since.as_millis()).expect("in range")
}

/// An entry's investigators as `(card code, holder, lead)`, in seating order.
fn investigators(entry: &GameEntry) -> Vec<(&str, AccountId, bool)> {
    entry
        .investigators
        .iter()
        .map(|i| (i.investigator.as_str(), i.holder, i.lead))
        .collect()
}

#[tokio::test]
async fn a_member_sees_the_lobby_with_its_picks_and_never_a_game_it_is_not_a_member_of() {
    let before = now_millis();
    let mut members = lobby::tal_and_sam().await;
    lobby::take_seen_by_both(&mut members.sam_ws, &mut members.tal_ws, DAISY).await;
    lobby::take_seen_by_both(&mut members.tal_ws, &mut members.sam_ws, ROLAND).await;
    let after = now_millis();
    // A second lobby of Tal's, whose link Sam never opens.
    let tal_only = lobby::create_lobby(members.addr, TEST_ACCOUNT).await;

    let listing = listing_for(members.addr, OTHER_ACCOUNT).await;

    let [entry] = listing.as_slice() else {
        panic!("Sam is a member of one game: {listing:?}");
    };
    assert_eq!(entry.game_id, GameId::new(members.game_id.as_str()));
    assert_eq!(entry.scenario_id.as_str(), TEST_SCENARIO_ID);
    assert_eq!(entry.creator, members.tal);
    assert_eq!(entry.state, GameEntryState::Lobby);
    assert_eq!(
        investigators(entry),
        [(DAISY, members.sam, true), (ROLAND, members.tal, false)],
        "the picks in seating order, Daisy leading as the first picked"
    );
    assert_eq!(entry.name(members.tal), Some(TEST_ACCOUNT));
    assert_eq!(entry.name(members.sam), Some(OTHER_ACCOUNT));
    assert!(
        (before..=after).contains(&entry.last_activity),
        "a lobby's last activity is when it was created: {}",
        entry.last_activity
    );

    let tal_listing = listing_for(members.addr, TEST_ACCOUNT).await;
    let mut tal_games: Vec<&str> = tal_listing.iter().map(|e| e.game_id.as_str()).collect();
    tal_games.sort_unstable();
    let mut expected = [members.game_id.as_str(), tal_only.as_str()];
    expected.sort_unstable();
    assert_eq!(tal_games, expected, "Tal is a member of both");
}

#[tokio::test]
async fn a_signed_out_request_is_refused() {
    let pool = common::memory_pool().await;
    let addr = common::spawn_server(pool).await;
    lobby::create_lobby(addr, TEST_ACCOUNT).await;

    let response = get_games(addr, None).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
