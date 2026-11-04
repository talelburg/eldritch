//! `GET /games`: the "Your games" listing of every game the signed-in account
//! is a member of, over real HTTP with two accounts.

use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use game_core::action::{InputResponse, PlayerAction};
use game_core::state::investigator::InvestigatorId;
use game_core::state::Phase;
use game_core::test_support;
use protocol::live::LiveSummary;
use protocol::{
    AccountId, ClientMessage, GameEntry, GameEntryState, GameId, LogPosition, ReplayCause,
    ServerMessage, UnresumableCause, UnresumableSummary,
};
use reqwest::StatusCode;
use scenarios::the_gathering::ID as GATHERING;
use sqlx::SqlitePool;

use crate::common::{self, Client, OTHER_ACCOUNT, TEST_ACCOUNT, TEST_SCENARIO_ID};
use crate::lobby::{self, AGNES, DAISY, ROLAND};

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
fn investigators(entry: &GameEntry) -> Vec<(&str, Option<AccountId>, bool)> {
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
    assert_eq!(entry.creator, Some(members.tal));
    assert_eq!(entry.state, GameEntryState::Lobby);
    assert_eq!(
        investigators(entry),
        [
            (DAISY, Some(members.sam), true),
            (ROLAND, Some(members.tal), false)
        ],
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

/// The next frame on `ws`, which must be `kind`.
async fn expect_frame(ws: &mut Client, kind: &str) {
    let frame = common::recv_within(ws).await;
    let seen = match &frame {
        ServerMessage::Hello { .. } => "Hello",
        ServerMessage::Applied(_) => "Applied",
        _ => "another frame",
    };
    assert_eq!(seen, kind, "{frame:?}");
}

/// The summary of a Live entry.
fn summary(entry: &GameEntry) -> &LiveSummary {
    let GameEntryState::Live(summary) = &entry.state else {
        panic!("expected a Live entry, got {:?}", entry.state);
    };
    summary
}

/// The investigators a summary waits on, as `(card code, holder, yours)`.
fn waited(summary: &LiveSummary) -> Vec<(&str, Option<AccountId>, bool)> {
    summary
        .waiting_on
        .iter()
        .map(|waited| (waited.card.as_str(), waited.holder, waited.yours))
        .collect()
}

/// Tal and Sam start The Gathering with Sam's Daisy leading, so the mulligan
/// asks Daisy first, then Tal's Roland once Sam keeps Daisy's hand.
#[tokio::test]
async fn a_started_game_is_live_with_who_it_waits_on_and_its_last_activity() {
    let mut members = lobby::tal_and_sam_at(server::REGISTRIES, GATHERING).await;
    lobby::daisy_roland_agnes(&mut members).await;
    let before_start = now_millis();
    common::send(&mut members.tal_ws, &ClientMessage::Start).await;
    expect_frame(&mut members.tal_ws, "Hello").await;
    expect_frame(&mut members.sam_ws, "Hello").await;
    let after_start = now_millis();

    let listing = listing_for(members.addr, OTHER_ACCOUNT).await;

    let [entry] = listing.as_slice() else {
        panic!("Sam is a member of one game: {listing:?}");
    };
    assert_eq!(entry.game_id, GameId::new(members.game_id.as_str()));
    assert_eq!(entry.scenario_id.as_str(), GATHERING);
    assert_eq!(entry.creator, Some(members.tal));
    assert_eq!(
        investigators(entry),
        [
            (DAISY, Some(members.sam), true),
            (ROLAND, Some(members.tal), false),
            (AGNES, Some(members.tal), false)
        ],
        "the seated investigators in seating order, Daisy leading"
    );
    assert_eq!(entry.name(members.tal), Some(TEST_ACCOUNT));
    assert_eq!(entry.name(members.sam), Some(OTHER_ACCOUNT));
    let sams = summary(entry);
    assert_eq!(waited(sams), [(DAISY, Some(members.sam), true)]);
    assert!(sams.needs_you(), "the mulligan asks Sam's Daisy");
    assert_eq!((sams.round, sams.phase), (1, Phase::Investigation));
    assert!(
        (before_start..=after_start).contains(&entry.last_activity),
        "with nothing logged, last activity is when the game started: {}",
        entry.last_activity
    );

    let before_answer = now_millis();
    common::send(
        &mut members.sam_ws,
        &ClientMessage::Submit {
            prompt: LogPosition(0),
            action: PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        },
    )
    .await;
    expect_frame(&mut members.sam_ws, "Applied").await;
    expect_frame(&mut members.tal_ws, "Applied").await;
    let after_answer = now_millis();

    let listing = listing_for(members.addr, OTHER_ACCOUNT).await;

    let [entry] = listing.as_slice() else {
        panic!("Sam is a member of one game: {listing:?}");
    };
    let sams = summary(entry);
    assert_eq!(waited(sams), [(ROLAND, Some(members.tal), false)]);
    assert!(!sams.needs_you(), "the mulligan now asks Tal's Roland");
    assert_eq!(sams.waiting_for(), [members.tal]);
    assert!(
        (before_answer..=after_answer).contains(&entry.last_activity),
        "last activity is when the last answer was logged: {}",
        entry.last_activity
    );
}

/// Make `account` a member of `game_id` in `pool`, as opening its link does.
async fn add_member(pool: &SqlitePool, game_id: &str, account: AccountId) {
    sqlx::query("INSERT INTO game_members (game_id, account_id) VALUES (?, ?)")
        .bind(game_id)
        .bind(account.0)
        .execute(pool)
        .await
        .expect("add a member");
}

/// A game of the mock scenario the [`TEST_ACCOUNT`] created, holds and is a
/// member of, with its mulligan answered, stored by the build
/// `"older-build"`. Returns the [`TEST_ACCOUNT`]'s id.
async fn played_by_an_older_build(pool: &SqlitePool, game_id: &str) -> AccountId {
    let tester = common::test_account(pool).await;
    let mut session = common::create_game(pool, game_id).await;
    session
        .apply(
            tester,
            session.live().prompt(),
            PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        )
        .await
        .expect("the mulligan is accepted");
    add_member(pool, game_id, tester).await;
    sqlx::query("UPDATE games SET created_build = 'older-build' WHERE game_id = ?")
        .bind(game_id)
        .execute(pool)
        .await
        .expect("set the build it was created on");
    tester
}

/// The game's logged mulligan is turned into a `Confirm`, which the engine
/// rejects for it, as a build whose engine changed would.
#[tokio::test]
async fn a_game_whose_log_no_longer_replays_is_listed_unresumable_with_where_it_stopped() {
    let pool = common::memory_pool().await;
    let before = now_millis();
    let tester = played_by_an_older_build(&pool, "stale").await;
    let after = now_millis();
    common::set_logged(&pool, "stale", 0, "$.response", "Confirm").await;
    let addr = common::spawn_server(pool).await;

    let listing = listing_for(addr, TEST_ACCOUNT).await;

    let [entry] = listing.as_slice() else {
        panic!("the Tester is a member of one game: {listing:?}");
    };
    assert_eq!(entry.game_id, GameId::new("stale"));
    assert_eq!(entry.scenario_id.as_str(), TEST_SCENARIO_ID);
    assert_eq!(entry.creator, Some(tester));
    assert_eq!(
        investigators(entry),
        [(test_support::TEST_INV, Some(tester), true)]
    );
    assert_eq!(entry.name(tester), Some(TEST_ACCOUNT));
    assert!(
        (before..=after).contains(&entry.last_activity),
        "last activity is when the last answer was logged: {}",
        entry.last_activity
    );
    assert_eq!(
        entry.state,
        GameEntryState::Unresumable(UnresumableSummary {
            reason: UnresumableCause::Replay {
                seq: 0,
                cause: ReplayCause::Rejected,
            },
            logged_answers: 1,
            created_build: "older-build".to_owned(),
            server_build: protocol::build_id().to_owned(),
        })
    );
}

/// A game whose assignment rows are gone: nobody holds its investigators.
#[tokio::test]
async fn an_unresumable_game_with_no_assignment_lists_its_investigators_with_no_holder() {
    let pool = common::memory_pool().await;
    let tester = played_by_an_older_build(&pool, "unheld").await;
    sqlx::query("DELETE FROM game_assignments WHERE game_id = 'unheld'")
        .execute(&pool)
        .await
        .expect("drop the assignment");
    let addr = common::spawn_server(pool).await;

    let listing = listing_for(addr, TEST_ACCOUNT).await;

    let [entry] = listing.as_slice() else {
        panic!("the Tester is a member of one game: {listing:?}");
    };
    assert_eq!(entry.creator, Some(tester));
    assert_eq!(investigators(entry), [(test_support::TEST_INV, None, true)]);
    let GameEntryState::Unresumable(summary) = &entry.state else {
        panic!("expected an Unresumable entry, got {:?}", entry.state);
    };
    assert_eq!(summary.reason, UnresumableCause::NoAssignment);
}
