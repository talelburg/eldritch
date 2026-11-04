//! Start, at the end of the seating: the creator's button, the line every
//! other member sees in its place, and the one-off strip a member present at
//! Start sees on the board.

use game_core::engine::EngineOutcome;
use game_core::state::{GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support;
use leptos::prelude::*;
use leptos::task;
use protocol::{Assignment, ClientMessage, LogPosition, ServerMessage};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web::store::{self, ClientState, ConnStatus};
use web_sys::HtmlButtonElement;

use super::{click, mount_lobby, one, sent, tals_lobby_with_picks, texts, SAM};
use crate::common::{self, YOU};

const ROLAND: &str = "01001";
const DAISY: &str = "01002";

#[wasm_bindgen_test]
async fn the_creator_starts_the_game_with_start() {
    let picks = [(DAISY, SAM), (ROLAND, YOU)];
    let mut handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    assert_eq!(
        texts(&handle, ".lobby button.start"),
        ["Starts with 2 investigators. The roster can't change after."]
    );
    assert!(texts(&handle, ".lobby .waiting").is_empty());

    click(&handle, ".lobby button.start");
    task::tick().await;
    assert!(matches!(sent(&mut handle), ClientMessage::Start));
}

#[wasm_bindgen_test]
async fn start_is_unavailable_until_there_is_a_pick() {
    let handle = mount_lobby(tals_lobby_with_picks(&[]), YOU, false).await;

    let start = one(&handle, ".lobby button.start")
        .dyn_into::<HtmlButtonElement>()
        .expect("a button");
    assert!(start.disabled(), "an empty game can't start");
}

#[wasm_bindgen_test]
async fn every_other_member_sees_whom_the_lobby_waits_on() {
    let picks = [(DAISY, SAM)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), SAM, false).await;

    assert_eq!(
        texts(&handle, ".lobby .waiting"),
        ["Waiting for Tal to start"]
    );
    assert!(texts(&handle, ".lobby button.start").is_empty());
}

/// The game Start seated from Sam's Daisy and Tal's Roland: Daisy in seat 1,
/// leading.
fn daisy_and_roland_seated() -> GameState {
    let mut daisy = test_support::test_investigator(1);
    daisy.name = "Daisy Walker".to_string();
    let mut roland = test_support::test_investigator(2);
    roland.name = "Roland Banks".to_string();
    GameStateBuilder::new()
        .with_investigator(daisy)
        .with_investigator(roland)
        .with_seating([InvestigatorId(1), InvestigatorId(2)])
        .with_lead(InvestigatorId(1))
        .build()
}

/// The `Hello` frame each connection to the started game is sent, for the
/// account `you`.
fn hello_after_start(you: protocol::AccountId) -> String {
    serde_json::to_string(&ServerMessage::Hello {
        build: protocol::build_id().to_string(),
        you,
        assigned: Assignment::from_iter([(InvestigatorId(1), SAM), (InvestigatorId(2), YOU)]),
        prompt: LogPosition(0),
        state: Box::new(daisy_and_roland_seated()),
        outcome: EngineOutcome::Done,
        events: Vec::new(),
    })
    .expect("a Hello serializes")
}

#[wasm_bindgen_test]
async fn a_member_at_start_sees_the_board_with_the_roster_and_its_lead() {
    let picks = [(DAISY, SAM), (ROLAND, YOU)];
    let handle = mount_lobby(tals_lobby_with_picks(&picks), YOU, false).await;

    handle
        .store
        .update(|s| _ = store::receive(s, &hello_after_start(YOU)));
    task::tick().await;

    assert_eq!(
        handle.store.with_untracked(|s| s.status.clone()),
        ConnStatus::Connected
    );
    assert!(texts(&handle, ".lobby").is_empty(), "the lobby is gone");
    assert_eq!(
        texts(&handle, ".start-strip .strip-seat"),
        ["Daisy Walker (Sam) ★", "Roland Banks (Tal)"],
        "the roster in seating order, each with its member, and the lead"
    );

    click(&handle, ".start-strip button.dismiss");
    task::tick().await;
    assert!(
        texts(&handle, ".start-strip").is_empty(),
        "the strip is one-off"
    );
}

#[wasm_bindgen_test]
async fn a_connection_opening_on_a_started_game_shows_no_strip() {
    let handle = common::mount_board(ClientState::default(), &cards::TEST_REGISTRY).await;

    handle
        .store
        .update(|s| _ = store::receive(s, &hello_after_start(SAM)));
    task::tick().await;

    assert_eq!(
        handle.store.with_untracked(|s| s.status.clone()),
        ConnStatus::Connected
    );
    assert!(texts(&handle, ".start-strip").is_empty());
}
