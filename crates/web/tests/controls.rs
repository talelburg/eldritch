//! Headless tests for `ActionControls` (P6.7a, post-2b #447): the only bespoke
//! control left is `StartScenario`. Open-turn gameplay is the engine's
//! `AwaitingInput` action menu, rendered by `AwaitingInputView` (covered by
//! `tests/awaiting_input.rs`). wasm32-only (browser DOM + wasm-only transport).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::GameStateBuilder;
use game_core::state::{InvestigatorId, Phase};
use game_core::test_support::fixtures::{awaiting_commit_input, test_investigator};
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

/// An in-progress Investigation-phase game (round 1, so never the round-0
/// pre-start state).
fn investigation_game() -> game_core::state::GameState {
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .with_phase(Phase::Investigation)
        .with_round(1)
        .build()
}

/// Extract the submitted `PlayerAction`. `Submit` is the only `ClientMessage`
/// variant, so the destructure is irrefutable.
fn submit_action(frame: ClientMessage) -> PlayerAction {
    let ClientMessage::Submit { action } = frame;
    action
}

#[wasm_bindgen_test]
async fn round_zero_renders_an_enabled_start_scenario_that_submits() {
    // The state straight from a scenario `setup()`: round 0. `StartScenario` is
    // the sole legal action, and clicking it submits the matching frame.
    let game = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    assert_eq!(game.round, 0, "precondition: pre-start state");

    let mut rx = mount(game, EngineOutcome::Done).await;
    let controls = last_controls();
    let start = controls
        .query_selector(".start-scenario")
        .expect("query")
        .expect("start-scenario present");
    assert!(
        !start.has_attribute("disabled"),
        "Start scenario should be enabled pre-start"
    );

    click_in(&controls, ".start-scenario");
    leptos::task::tick().await;
    match submit_action(rx.try_recv().expect("a frame after tick")) {
        PlayerAction::StartScenario { .. } => {}
        other => panic!("expected StartScenario, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn no_bespoke_in_game_action_controls_render() {
    // Post-2b (#447) there are no per-action buttons: open-turn gameplay is the
    // engine's `AwaitingInput` action menu (rendered by `AwaitingInputView`).
    // None of the old bespoke controls exist in the section.
    let _rx = mount(investigation_game(), EngineOutcome::Done).await;
    let controls = last_controls();
    for gone in [
        ".end-turn",
        ".investigate",
        ".advance-act",
        ".draw",
        ".move-dest",
        ".play-card",
        ".fight-target",
        ".evade-target",
    ] {
        assert!(
            controls.query_selector(gone).expect("query").is_none(),
            "the {gone} bespoke control must be gone (#447)"
        );
    }
}

#[wasm_bindgen_test]
async fn start_scenario_disabled_and_inert_during_awaiting_input() {
    // During an `AwaitingInput` pause (the open-turn menu, a commit window, …)
    // `enabled_controls` returns empty, so the start-scenario button is disabled
    // and a click submits nothing.
    let mut rx = mount(investigation_game(), awaiting_commit_input("commit")).await;
    let controls = last_controls();
    let start = controls
        .query_selector(".start-scenario")
        .expect("query")
        .expect("start-scenario present");
    assert!(
        start.has_attribute("disabled"),
        "start-scenario should be disabled during an AwaitingInput pause"
    );

    click_in(&controls, ".start-scenario");
    leptos::task::tick().await;
    assert!(rx.try_recv().is_err(), "disabled button must not submit");
}

#[wasm_bindgen_test]
async fn resolved_scenario_disables_start_scenario() {
    use game_core::Resolution;
    // A round-0 game (where StartScenario would otherwise be enabled); once a
    // resolution latches, the control is disabled.
    let mut game = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    game.resolution = Some(Resolution::Won { id: "demo".into() });

    let _rx = mount(game, EngineOutcome::Done).await;
    let start = last_controls()
        .query_selector(".start-scenario")
        .expect("query")
        .expect("start-scenario present");
    assert!(
        start.has_attribute("disabled"),
        "start-scenario should be disabled when the scenario is resolved"
    );
}
