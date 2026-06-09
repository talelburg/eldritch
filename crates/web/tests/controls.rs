//! Headless tests for `ActionControls` (P6.7a): feed a `GameState` +
//! `EngineOutcome` through the store, then assert each control, when
//! legal, submits the matching `ClientMessage::Submit { action }`.
//! wasm32-only (browser DOM + the wasm-only transport types).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::builder::TestGame;
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::{EngineOutcome, PlayerAction};
use leptos::prelude::*;
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::controls::ActionControls;
use web::store::{reduce, ClientState};
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `ActionControls` with a fresh store + outbound channel, feed one
/// `Hello { state, outcome }`, tick, and return the receiver.
async fn mount(
    state: game_core::state::GameState,
    outcome: EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        leptos::view! { <ActionControls/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome,
            },
        );
    });
    leptos::task::tick().await;
    rx
}

/// The last mounted `.controls` section (DOM accumulates across tests).
fn last_controls() -> web_sys::Element {
    let secs = leptos::prelude::document()
        .query_selector_all(".controls")
        .expect("query");
    secs.item(secs.length() - 1)
        .expect("at least one controls section")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
}

/// Click the first element matching `selector` within `section`.
fn click_in(section: &web_sys::Element, selector: &str) {
    section
        .query_selector(selector)
        .expect("query")
        .expect("element present")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
}

/// An Investigation-phase game with one active investigator.
fn investigation_game() -> game_core::state::GameState {
    TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .build()
}

/// Extract the submitted `PlayerAction`. `ClientMessage` has no
/// `PartialEq`, so tests match the action rather than `assert_eq!` the
/// whole frame (the `input.rs` pattern). `Submit` is the only variant, so
/// the destructure is irrefutable.
fn submit_action(frame: ClientMessage) -> PlayerAction {
    let ClientMessage::Submit { action } = frame;
    action
}

#[wasm_bindgen_test]
async fn end_turn_submits_end_turn() {
    let mut rx = mount(investigation_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".end-turn");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::EndTurn => {}
        other => panic!("expected EndTurn, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn investigate_submits_investigate_for_active() {
    let mut rx = mount(investigation_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".investigate");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::Investigate { investigator } => {
            assert_eq!(investigator, InvestigatorId(1));
        }
        other => panic!("expected Investigate, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn advance_act_submits_advance_act_for_active() {
    let mut rx = mount(investigation_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".advance-act");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::AdvanceAct { investigator } => {
            assert_eq!(investigator, InvestigatorId(1));
        }
        other => panic!("expected AdvanceAct, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn draw_encounter_submits_draw_encounter_card() {
    // Mythos phase with this investigator on the draw cursor: only
    // DrawEncounter is legal. `mythos_draw_pending` has no builder setter,
    // so mutate the built state directly (legality.rs tests do the same).
    let mut game = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Mythos)
        .build();
    game.mythos_draw_pending = Some(InvestigatorId(1));

    let mut rx = mount(game, EngineOutcome::Done).await;
    click_in(&last_controls(), ".draw-encounter");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::DrawEncounterCard => {}
        other => panic!("expected DrawEncounterCard, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn illegal_controls_are_disabled_and_do_not_submit() {
    // Mythos + draw cursor: only DrawEncounter is legal, so End turn is
    // disabled and DrawEncounter is not.
    let mut game = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Mythos)
        .build();
    game.mythos_draw_pending = Some(InvestigatorId(1));

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    let end_turn = controls
        .query_selector(".end-turn")
        .expect("query")
        .expect("end-turn present");
    let draw = controls
        .query_selector(".draw-encounter")
        .expect("query")
        .expect("draw-encounter present");
    assert!(
        end_turn.has_attribute("disabled"),
        "End turn should be disabled"
    );
    assert!(
        !draw.has_attribute("disabled"),
        "Draw encounter should be enabled"
    );

    // A disabled button does not fire click → no frame.
    click_in(&controls, ".end-turn");
    leptos::task::tick().await;
    assert!(rx.try_recv().is_err(), "disabled button must not submit");
}

#[wasm_bindgen_test]
async fn move_picker_submits_move_to_connected_destination() {
    // Investigator at location 1, which connects to location 2.
    let mut loc1 = test_location(1, "Study");
    loc1.connections = vec![LocationId(2)];
    let loc2 = test_location(2, "Hallway");
    let game = TestGame::new()
        .with_investigator_at(test_investigator(1), LocationId(1))
        .with_active_investigator(InvestigatorId(1))
        .with_location(loc1)
        .with_location(loc2)
        .with_phase(Phase::Investigation)
        .build();

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    // The single destination button is labeled by the destination name.
    let dest = controls
        .query_selector(".move-dest")
        .expect("query")
        .expect("a move destination button");
    assert!(dest.text_content().unwrap_or_default().contains("Hallway"));

    click_in(&controls, ".move-dest");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::Move {
            investigator,
            destination,
        } => {
            assert_eq!(investigator, InvestigatorId(1));
            assert_eq!(destination, LocationId(2));
        }
        other => panic!("expected Move, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn play_picker_submits_play_card_by_hand_index() {
    let mut inv = test_investigator(1);
    inv.hand = vec![
        CardCode::new("_synth_event_a"),
        CardCode::new("_synth_event_b"),
    ];
    let game = TestGame::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .build();

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    // Click the second card's Play button → hand_index 1.
    let buttons = controls.query_selector_all(".play-card").expect("query");
    buttons
        .item(1)
        .expect("second play button")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::PlayCard {
            investigator,
            hand_index,
        } => {
            assert_eq!(investigator, InvestigatorId(1));
            assert_eq!(hand_index, 1);
        }
        other => panic!("expected PlayCard, got {other:?}"),
    }
}

/// A setup game where investigator 1 is on the mulligan cursor with a
/// two-card hand.
fn mulligan_game() -> game_core::state::GameState {
    let mut inv = test_investigator(1);
    inv.hand = vec![
        CardCode::new("_synth_event_a"),
        CardCode::new("_synth_event_b"),
    ];
    TestGame::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .with_mulligan_pending(InvestigatorId(1))
        .build()
}

#[wasm_bindgen_test]
async fn mulligan_submits_selected_indices() {
    let mut rx = mount(mulligan_game(), EngineOutcome::Done).await;
    let controls = last_controls();
    // Select the first card, then submit.
    let cards = controls.query_selector_all(".mull-card").expect("query");
    cards
        .item(0)
        .expect("first mull-card")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;
    click_in(&controls, ".mulligan-submit");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::Mulligan {
            investigator,
            indices_to_redraw,
        } => {
            assert_eq!(investigator, InvestigatorId(1));
            assert_eq!(indices_to_redraw, vec![0]);
        }
        other => panic!("expected Mulligan, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn mulligan_with_no_selection_keeps_hand() {
    let mut rx = mount(mulligan_game(), EngineOutcome::Done).await;
    click_in(&last_controls(), ".mulligan-submit");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::Mulligan {
            investigator,
            indices_to_redraw,
        } => {
            assert_eq!(investigator, InvestigatorId(1));
            assert_eq!(indices_to_redraw, Vec::<u8>::new());
        }
        other => panic!("expected Mulligan, got {other:?}"),
    }
}
