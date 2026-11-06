//! Websocket hub: connect → `Hello`, accepted submits broadcast
//! `Applied` to every connection, rejected submits return `Rejected`
//! to the sender only.

use std::time::Duration;

use axum::extract::ws::close_code;
use futures_util::SinkExt as _;
use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, OptionId};
use game_core::rng::RngState;
use game_core::state::investigator::InvestigatorId;
use protocol::{
    Applied, ClientMessage, LogPosition, Refusal, ServerMessage, CLOSE_GAME_NOT_FOUND,
    CLOSE_SERVER_RESTARTING, CLOSE_UNRESUMABLE,
};
use server::shutdown::Shutdown;
use tokio::time;
use tokio_tungstenite::tungstenite::Message;

use crate::common::{self, OTHER_ACCOUNT};

#[tokio::test]
async fn connect_receives_hello_with_current_state() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-hello").await;
    let addr = common::spawn_server(pool).await;

    let mut ws = common::connect(addr, "g-hello").await;

    match common::recv(&mut ws).await {
        ServerMessage::Hello { state, outcome, .. } => {
            // create seats the roster: round is 1, mulligan is pending.
            assert_eq!(state.round, 1);
            assert!(
                matches!(outcome, EngineOutcome::AwaitingInput { .. }),
                "freshly-created game is mulligan-pending, got {outcome:?}"
            );
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn connect_to_a_missing_game_is_closed_as_not_found_and_a_real_game_still_attaches() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-real").await;
    let addr = common::spawn_server(pool).await;

    let mut missing = common::connect(addr, "g-missing").await;
    common::assert_closed_with(&mut missing, CLOSE_GAME_NOT_FOUND).await;

    let mut real = common::connect(addr, "g-real").await;
    match common::recv(&mut real).await {
        ServerMessage::Hello { .. } => {}
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[tokio::test]
async fn connect_to_an_unresumable_game_is_closed_as_unresumable() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-unresumable").await;
    // A stored format version this build doesn't accept.
    common::set_format_version(&pool, "g-unresumable", 99).await;
    let addr = common::spawn_server(pool).await;

    let mut ws = common::connect(addr, "g-unresumable").await;
    common::assert_closed_with(&mut ws, CLOSE_UNRESUMABLE).await;
}

#[tokio::test]
async fn accepted_action_broadcasts_applied_to_all_clients() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-bcast").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-bcast").await;
    let mut b = common::connect(addr, "g-bcast").await;
    // Draining each Hello guarantees both connections have subscribed
    // before the submit, so neither misses the broadcast.
    let _ = common::recv(&mut a).await;
    let _ = common::recv(&mut b).await;

    // Resolve the setup mulligan (keep the full hand — empty redraw).
    common::send(
        &mut a,
        &ClientMessage::Submit {
            prompt: LogPosition(0),
            action: PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple { selected: vec![] },
            },
        },
    )
    .await;

    for ws in [&mut a, &mut b] {
        match common::recv(ws).await {
            ServerMessage::Applied(Applied {
                outcome, events, ..
            }) => {
                assert!(!matches!(outcome, EngineOutcome::Rejected { .. }));
                assert!(
                    matches!(outcome, EngineOutcome::AwaitingInput { .. } | EngineOutcome::Done),
                    "resolving the mulligan advances to the open-turn menu or completes, got {outcome:?}"
                );
                assert!(!events.is_empty(), "resolving the mulligan emits events");
            }
            other => panic!("expected Applied, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn rejected_action_returns_rejected_to_sender_only() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-reject").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-reject").await;
    let mut b = common::connect(addr, "g-reject").await;
    let _ = common::recv(&mut a).await;
    let _ = common::recv(&mut b).await;

    // Post-create the mulligan is pending. Selecting a non-existent hand
    // index (OptionId(999_999)) is rejected by the mulligan handler.
    common::send(
        &mut a,
        &ClientMessage::Submit {
            prompt: LogPosition(0),
            action: PlayerAction::ResolveInput {
                investigator: InvestigatorId(1),
                response: InputResponse::PickMultiple {
                    selected: vec![OptionId(999_999)],
                },
            },
        },
    )
    .await;

    match common::recv(&mut a).await {
        ServerMessage::Rejected(Refusal::Engine(_)) => {}
        other => panic!("expected an engine Rejected, got {other:?}"),
    }

    // B sees nothing: rejections are not broadcast.
    let quiet = time::timeout(Duration::from_millis(200), common::recv(&mut b)).await;
    assert!(
        quiet.is_err(),
        "B must receive nothing for a rejected action"
    );
}

/// The log position a `Hello` names.
fn hello_prompt(msg: ServerMessage) -> LogPosition {
    match msg {
        ServerMessage::Hello { prompt, .. } => prompt,
        other => panic!("expected Hello, got {other:?}"),
    }
}

/// The mulligan's answer that keeps the whole opening hand, for the prompt at
/// `prompt`.
fn keep_opening_hand(prompt: LogPosition) -> ClientMessage {
    ClientMessage::Submit {
        prompt,
        action: PlayerAction::ResolveInput {
            investigator: InvestigatorId(1),
            response: InputResponse::PickMultiple { selected: vec![] },
        },
    }
}

/// Require the next message on `ws` to be the `Members` that the
/// [`OTHER_ACCOUNT`] opening the game's link brings: the fixture's game stores
/// no member, so the first time an account connects it joins.
async fn recv_other_joined(ws: &mut common::Client) {
    match common::recv_within(ws).await {
        ServerMessage::Members(members) => assert_eq!(
            members.last().map(|member| member.display_name.as_str()),
            Some(OTHER_ACCOUNT)
        ),
        other => panic!("expected Members, got {other:?}"),
    }
}

/// Require the next message on `ws` to be a `StalePrompt` refusal.
async fn assert_stale(ws: &mut common::Client) {
    match common::recv_within(ws).await {
        ServerMessage::Rejected(Refusal::StalePrompt) => {}
        other => panic!("expected a StalePrompt refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_second_tab_answering_the_prompt_already_answered_is_refused_as_stale() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-stale").await;
    let addr = common::spawn_server(pool.clone()).await;

    // Two tabs of one account, both shown the mulligan.
    let mut a = common::connect(addr, "g-stale").await;
    let mut b = common::connect(addr, "g-stale").await;
    let a_prompt = hello_prompt(common::recv(&mut a).await);
    let b_prompt = hello_prompt(common::recv(&mut b).await);
    assert_eq!(a_prompt, b_prompt, "both tabs are shown the same prompt");

    // Tab A answers it, and both tabs are sent the Applied.
    common::send(&mut a, &keep_opening_hand(a_prompt)).await;
    for ws in [&mut a, &mut b] {
        match common::recv_within(ws).await {
            ServerMessage::Applied(applied) => assert_eq!(applied.answered, a_prompt),
            other => panic!("expected Applied, got {other:?}"),
        }
    }

    // Tab B, before it has redrawn, sends the same answer to the same prompt.
    common::send(&mut b, &keep_opening_hand(b_prompt)).await;

    assert_stale(&mut b).await;
    assert_eq!(
        common::stored_rows(&pool, "g-stale").await,
        (1, 1),
        "only tab A's answer is stored"
    );
}

#[tokio::test]
async fn a_stale_answer_from_another_account_is_refused_as_stale() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-stale-other").await;
    common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool.clone()).await;

    let mut tester = common::connect(addr, "g-stale-other").await;
    let mut other = common::connect_as(addr, "g-stale-other", OTHER_ACCOUNT).await;
    let prompt = hello_prompt(common::recv(&mut tester).await);
    let _ = common::recv(&mut other).await;
    recv_other_joined(&mut tester).await;
    common::send(&mut tester, &keep_opening_hand(prompt)).await;
    let _ = common::recv_within(&mut tester).await;
    let _ = common::recv_within(&mut other).await;

    // The position is checked first, whoever sends it.
    common::send(&mut other, &keep_opening_hand(prompt)).await;

    assert_stale(&mut other).await;
    assert_eq!(common::stored_rows(&pool, "g-stale-other").await, (1, 1));
}

#[tokio::test]
async fn another_accounts_answer_is_refused_as_not_yours_and_nothing_is_stored() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-not-yours").await;
    common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool.clone()).await;

    // The creator holds the one investigator, so the other account is a
    // viewer.
    let mut other = common::connect_as(addr, "g-not-yours", OTHER_ACCOUNT).await;
    let prompt = hello_prompt(common::recv(&mut other).await);
    common::send(&mut other, &keep_opening_hand(prompt)).await;

    match common::recv_within(&mut other).await {
        ServerMessage::Rejected(Refusal::NotYours) => {}
        other => panic!("expected a NotYours refusal, got {other:?}"),
    }
    assert_eq!(common::stored_rows(&pool, "g-not-yours").await, (1, 0));
}

#[tokio::test]
async fn hello_names_each_account_and_the_creator_holds_every_investigator() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-assigned").await;
    let creator = common::test_account(&pool).await;
    let other = common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool).await;

    let mut tester_ws = common::connect(addr, "g-assigned").await;
    let mut other_ws = common::connect_as(addr, "g-assigned", OTHER_ACCOUNT).await;

    for (ws, account) in [(&mut tester_ws, creator), (&mut other_ws, other)] {
        let ServerMessage::Hello {
            you,
            assigned,
            state,
            ..
        } = common::recv(ws).await
        else {
            panic!("expected Hello");
        };
        assert_eq!(you, account, "Hello names the connecting account");
        assert!(!state.investigators.is_empty());
        for investigator in state.investigators.keys() {
            assert_eq!(
                assigned.holder(*investigator),
                Some(creator),
                "the creator holds {investigator:?}"
            );
        }
        assert_eq!(state.rng, RngState::new(0), "Hello carries no RNG state");
    }
}

#[tokio::test]
async fn every_account_is_sent_the_answer_with_no_rng_state() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-per-account").await;
    common::add_account(&pool, OTHER_ACCOUNT).await;
    let addr = common::spawn_server(pool).await;

    let mut tester = common::connect(addr, "g-per-account").await;
    let mut other = common::connect_as(addr, "g-per-account", OTHER_ACCOUNT).await;
    let prompt = hello_prompt(common::recv(&mut tester).await);
    let _ = common::recv(&mut other).await;
    recv_other_joined(&mut tester).await;
    common::send(&mut tester, &keep_opening_hand(prompt)).await;

    for ws in [&mut tester, &mut other] {
        let ServerMessage::Applied(Applied { by, state, .. }) = common::recv_within(ws).await
        else {
            panic!("expected Applied");
        };
        assert_eq!(by, InvestigatorId(1), "the Applied names who answered");
        assert_eq!(state.rng, RngState::new(0), "Applied carries no RNG state");
    }
}

#[tokio::test]
async fn a_frame_that_does_not_parse_is_refused_as_malformed() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-malformed").await;
    let addr = common::spawn_server(pool).await;

    let mut ws = common::connect(addr, "g-malformed").await;
    let _ = common::recv(&mut ws).await;
    ws.send(Message::Text("{\"submit\":{}}".into()))
        .await
        .expect("ws send");

    match common::recv_within(&mut ws).await {
        ServerMessage::Rejected(Refusal::Malformed) => {}
        other => panic!("expected a Malformed refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_ping_is_answered_with_a_pong_to_that_socket_only() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-ping").await;
    let addr = common::spawn_server(pool).await;

    let mut a = common::connect(addr, "g-ping").await;
    let mut b = common::connect(addr, "g-ping").await;
    let _ = common::recv(&mut a).await;
    let _ = common::recv(&mut b).await;

    common::send(&mut a, &ClientMessage::Ping).await;

    assert_pong(&mut a).await;
    let quiet = time::timeout(Duration::from_millis(200), common::recv(&mut b)).await;
    assert!(quiet.is_err(), "B must receive nothing for A's ping");
}

/// The silence timeout the liveness tests inject in place of
/// [`protocol::SERVER_SILENCE_TIMEOUT`].
const SHORT_SILENCE: Duration = Duration::from_millis(300);

/// Require the next message on `ws` to be a `Pong`, within a bound so a missing
/// one fails the test rather than hanging it.
async fn assert_pong(ws: &mut common::Client) {
    match common::recv_within(ws).await {
        ServerMessage::Pong => {}
        other => panic!("expected Pong, got {other:?}"),
    }
}

#[tokio::test]
async fn a_socket_that_sends_nothing_is_closed_as_going_away() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-silent").await;
    let addr =
        common::spawn_server_with(pool, |state| state.with_silence_timeout(SHORT_SILENCE)).await;

    let mut ws = common::connect(addr, "g-silent").await;
    let _ = common::recv(&mut ws).await;

    common::assert_closed_with(&mut ws, close_code::AWAY).await;
}

#[tokio::test]
async fn a_socket_that_keeps_pinging_stays_open_past_the_timeout() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-pinging").await;
    let addr =
        common::spawn_server_with(pool, |state| state.with_silence_timeout(SHORT_SILENCE)).await;

    let mut ws = common::connect(addr, "g-pinging").await;
    let _ = common::recv(&mut ws).await;

    // Four pings half a timeout apart span twice the timeout, and each one is
    // still answered.
    for _ in 0..4 {
        time::sleep(SHORT_SILENCE / 2).await;
        common::send(&mut ws, &ClientMessage::Ping).await;
        assert_pong(&mut ws).await;
    }
}

#[tokio::test]
async fn the_shutdown_trigger_closes_every_open_socket_as_restarting() {
    let pool = common::memory_pool().await;
    common::create_game(&pool, "g-one").await;
    common::create_game(&pool, "g-two").await;
    let shutdown = Shutdown::new();
    let trigger = shutdown.clone();
    let addr = common::spawn_server_with(pool, |state| state.with_shutdown(trigger)).await;

    // Two sockets on one game and one on another, each past its `Hello`.
    let mut sockets = Vec::new();
    for game_id in ["g-one", "g-one", "g-two"] {
        let mut ws = common::connect(addr, game_id).await;
        let _ = common::recv_within(&mut ws).await;
        sockets.push(ws);
    }

    shutdown.trigger();

    for ws in &mut sockets {
        common::assert_closed_with(ws, CLOSE_SERVER_RESTARTING).await;
    }
}
