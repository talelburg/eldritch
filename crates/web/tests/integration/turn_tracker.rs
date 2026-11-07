//! Headless render tests for the turn tracker. wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::{GameStateBuilder, Phase, RoundPosition};
use game_core::test_support;
use leptos::prelude::{document, provide_context, RwSignal, Update};
use leptos::{mount, task};
use protocol::{LogPosition, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::store::{self, ClientState};
use web::turn_tracker::TurnTrackerView;
use web_sys::Element;

use crate::common;

async fn mount_at(phase: Phase, round: u32, position: Option<RoundPosition>) {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(phase)
        .with_round(round)
        .build();
    state.round_position = position;
    let store = RwSignal::new(ClientState::default());
    mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <TurnTrackerView/> }
    });
    store.update(|s| {
        store::reduce(
            s,
            ServerMessage::Hello {
                build: protocol::build_id().to_string(),
                you: common::YOU,
                creator: common::YOU,
                members: vec![common::you_member()],
                assigned: common::you_hold(Some(&state), None),
                prompt: LogPosition(0),
                state: Box::new(state),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
    });
    task::tick().await;
}

fn last_tracker() -> Element {
    let nodes = document()
        .query_selector_all(".turn-tracker")
        .expect("query ok");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .turn-tracker")
}

/// The text of each `.tracker-step` and `.tracker-window` in the band, in
/// order.
fn band_entries(tracker: &Element) -> Vec<String> {
    common::texts_in(tracker, ".tracker-step, .tracker-window")
}

#[wasm_bindgen_test]
async fn lists_all_phases_substeps_and_round() {
    mount_at(Phase::Investigation, 2, Some(RoundPosition::TakeAction)).await;
    let tracker = last_tracker();
    let round = tracker
        .query_selector(".tracker-round")
        .expect("query ok")
        .expect("a .tracker-round")
        .text_content()
        .unwrap_or_default();
    assert_eq!(round, "R2", "the round, compactly");
    assert_eq!(
        band_entries(&tracker),
        [
            "1.1", "1.2", "1.3", "1.4", "◇", "1.5", //
            "2.1", "◇", "2.2", "◇", "2.2.1", "2.2.2", "2.3", //
            "3.1", "3.2", "◇", "3.3", "◇", "3.4", //
            "4.1", "◇", "4.2", "4.3", "4.4", "4.5", "4.6",
        ],
        "every step by number, ◇ for each player window"
    );
    let t = tracker.text_content().unwrap_or_default();
    assert!(
        !t.contains("Place 1 doom on the current agenda."),
        "a step's text waits for the hover: {t}"
    );
}

#[wasm_bindgen_test]
async fn hovering_a_step_shows_its_text() {
    mount_at(Phase::Mythos, 1, None).await;
    let tracker = last_tracker();
    let steps = tracker
        .query_selector_all(".tracker-step")
        .expect("query ok");
    let doom = (0..steps.length())
        .filter_map(|i| steps.item(i)?.dyn_into::<Element>().ok())
        .find(|el| el.text_content().unwrap_or_default() == "1.2")
        .expect("step 1.2");
    assert_eq!(common::popover_text(&doom), None, "closed before the hover");
    common::hover(&doom).await;
    let text = common::popover_text(&doom).expect("the step's popover");
    assert!(
        text.contains("Place 1 doom on the current agenda."),
        "the step's full text: {text}"
    );
    common::unhover(&doom).await;
    assert_eq!(common::popover_text(&doom), None, "closed after the hover");
}

#[wasm_bindgen_test]
async fn hovering_a_player_window_names_it() {
    mount_at(Phase::Mythos, 1, None).await;
    let window = last_tracker()
        .query_selector(".tracker-window")
        .expect("query ok")
        .expect("a .tracker-window");
    common::hover(&window).await;
    let text = common::popover_text(&window).expect("the window's popover");
    assert!(text.contains("Player window"), "names the window: {text}");
}

#[wasm_bindgen_test]
async fn the_current_entry_and_its_phase_are_highlighted() {
    mount_at(Phase::Enemy, 1, Some(RoundPosition::EnemyAttacks)).await;
    let tracker = last_tracker();
    // Exactly one phase block carries `current`, and it is the Enemy block.
    let currents = tracker
        .query_selector_all(".tracker-phase.current")
        .expect("query ok");
    assert_eq!(currents.length(), 1, "exactly one phase should be current");
    let current = tracker
        .query_selector(".tracker-phase.current")
        .expect("query ok")
        .expect("a current phase block")
        .text_content()
        .unwrap_or_default();
    assert!(
        current.contains("3.1") && current.contains("3.4"),
        "current phase should be Enemy's steps: {current}"
    );
    assert!(
        !current.contains("1.1") && !current.contains("2.1"),
        "only the Enemy block is current: {current}"
    );
    // Exactly one entry carries `current`: the step the round stands at.
    assert_eq!(
        common::texts_in(&tracker, ".tracker-step.current, .tracker-window.current"),
        ["3.3"],
    );
}

/// During the setup mulligan the round hasn't begun, so nothing is lit, though
/// setup has already set the Investigation phase.
#[wasm_bindgen_test]
async fn nothing_is_highlighted_before_the_round_begins() {
    mount_at(Phase::Investigation, 1, None).await;
    let lit = last_tracker()
        .query_selector_all(".current")
        .expect("query ok")
        .length();
    assert_eq!(lit, 0, "no phase and no entry is current during setup");
}
