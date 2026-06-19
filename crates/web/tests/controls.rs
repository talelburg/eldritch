//! Headless tests for `ActionControls` (P6.7a): feed a `GameState` +
//! `EngineOutcome` through the store, then assert each control, when
//! legal, submits the matching `ClientMessage::Submit { action }`.
//! wasm32-only (browser DOM + the wasm-only transport types).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::GameStateBuilder;
use game_core::state::{CardCode, EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::fixtures::{
    awaiting_commit_input, test_enemy, test_investigator, test_location,
};
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

/// An in-progress Investigation-phase game with one active investigator.
/// `round 1` because an in-progress game is never round 0 (round 0 is the
/// pre-start state that gates to `StartScenario`).
fn investigation_game() -> game_core::state::GameState {
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .with_round(1)
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

// The former `draw_encounter_submits_draw_encounter_card` test is gone (#348
// part 2c-iii-b): the dedicated Draw-encounter button was removed. The Mythos
// step-1.4 draw is now an `AwaitingInput(Confirm)` that flows through the
// `ResolveInput` prompt UI (Confirm/Skip rendering deferred to #205), not a
// core-loop control.

#[wasm_bindgen_test]
async fn illegal_controls_are_disabled_and_do_not_submit() {
    // During an `AwaitingInput` pause, every core-loop control is disabled
    // (`enabled_controls` returns empty). A disabled button must not submit a
    // frame when clicked.
    let game = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .with_round(1)
        .build();

    let mut rx = mount(game, awaiting_commit_input("commit")).await;
    let controls = last_controls();
    let end_turn = controls
        .query_selector(".end-turn")
        .expect("query")
        .expect("end-turn present");
    assert!(
        end_turn.has_attribute("disabled"),
        "End turn should be disabled during an AwaitingInput pause"
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
    let game = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(1))
        .with_active_investigator(InvestigatorId(1))
        .with_location(loc1)
        .with_location(loc2)
        .with_phase(Phase::Investigation)
        .with_round(1)
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
    let game = GameStateBuilder::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .with_round(1)
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

// The dedicated mulligan-picker tests are gone (#348 part 2c-iii-a): the setup
// mulligan is now an `AwaitingInput` handled by `AwaitingInputView`, covered by
// the mulligan tests in tests/input.rs.

/// An Investigation-phase game with one enemy (id 7) engaged with the
/// active investigator (id 1).
fn investigation_game_with_engaged_enemy() -> game_core::state::GameState {
    let mut enemy = test_enemy(7, "Ghoul");
    enemy.engaged_with = Some(InvestigatorId(1));
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .with_round(1)
        .with_enemy(enemy)
        .build()
}

#[wasm_bindgen_test]
async fn draw_button_submits_draw() {
    let game = investigation_game();
    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    click_in(&controls, ".action.draw");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::Draw { investigator } => {
            assert_eq!(investigator, InvestigatorId(1));
        }
        other => panic!("expected Draw, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn fight_target_submits_fight_with_chosen_enemy() {
    let game = investigation_game_with_engaged_enemy();
    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    click_in(&controls, ".fight-target");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::Fight {
            investigator,
            enemy,
        } => {
            assert_eq!(investigator, InvestigatorId(1));
            assert_eq!(enemy, EnemyId(7));
        }
        other => panic!("expected Fight, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn evade_target_submits_evade_with_chosen_enemy() {
    let game = investigation_game_with_engaged_enemy();
    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    click_in(&controls, ".evade-target");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::Evade {
            investigator,
            enemy,
        } => {
            assert_eq!(investigator, InvestigatorId(1));
            assert_eq!(enemy, EnemyId(7));
        }
        other => panic!("expected Evade, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn start_scenario_is_the_only_control_at_round_zero() {
    // The state straight from a scenario `setup()`: phase Mythos, round 0,
    // no cursors, no active investigator. The only legal action is
    // StartScenario — this is the state the server hands a freshly created
    // game, so the player must be able to start it.
    let game = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    assert_eq!(game.round, 0, "precondition: pre-start state");

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    let end_turn = controls
        .query_selector(".end-turn")
        .expect("query")
        .expect("end-turn present");
    let start = controls
        .query_selector(".start-scenario")
        .expect("query")
        .expect("start-scenario present");
    assert!(
        end_turn.has_attribute("disabled"),
        "core-loop buttons should be disabled pre-start"
    );
    assert!(
        !start.has_attribute("disabled"),
        "Start scenario should be enabled pre-start"
    );

    click_in(&controls, ".start-scenario");
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after tick");
    match submit_action(frame) {
        PlayerAction::StartScenario { .. } => {}
        other => panic!("expected StartScenario, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn resolved_scenario_disables_all_controls() {
    use game_core::Resolution;
    // An Investigation-phase game that would normally enable the core
    // loop; once resolved, every rendered control is disabled.
    let mut game = investigation_game();
    game.resolution = Some(Resolution::Won { id: "demo".into() });

    let _rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    // `.draw-encounter` is gone (#348 2c-iii-b); `.draw` is a representative
    // remaining core-loop button.
    for selector in [".start-scenario", ".end-turn", ".draw"] {
        let btn = controls
            .query_selector(selector)
            .expect("query")
            .expect("button present");
        assert!(
            btn.has_attribute("disabled"),
            "{selector} should be disabled when the scenario is resolved"
        );
    }
}
