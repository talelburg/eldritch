//! Start, at the end of the seating: the creator's button, the line every
//! other member sees in its place, and the one-off strip a member present at
//! Start sees on the board.

use game_core::engine::EngineOutcome;
use game_core::state::{GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support;
use leptos::prelude::*;
use leptos::task;
use protocol::{AccountId, Assignment, ClientMessage, LogPosition, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::store::{self, ClientState, ConnStatus};
use web_sys::HtmlButtonElement;

use super::{click, mount_lobby, sent, tals_lobby_with_picks, texts, DAISY, ROLAND, SAM};
use crate::common::{self, YOU};

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
async fn start_with_one_pick_names_one_investigator() {
    let handle = mount_lobby(tals_lobby_with_picks(&[(ROLAND, YOU)]), YOU, false).await;

    assert_eq!(
        texts(&handle, ".lobby button.start"),
        ["Starts with 1 investigator. The roster can't change after."]
    );
}

#[wasm_bindgen_test]
async fn start_is_unavailable_until_there_is_a_pick() {
    let handle = mount_lobby(tals_lobby_with_picks(&[]), YOU, false).await;

    let start = common::one(&handle.root, ".lobby button.start")
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
fn hello_after_start(you: AccountId) -> String {
    let lobby = tals_lobby_with_picks(&[]);
    serde_json::to_string(&ServerMessage::Hello {
        build: protocol::build_id().to_string(),
        you,
        creator: lobby.creator,
        members: lobby.members,
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
        texts(&handle, ".start-strip .strip-entry"),
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
