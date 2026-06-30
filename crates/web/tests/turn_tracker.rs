//! Headless render tests for the turn tracker. wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::state::{GameStateBuilder, Phase};
use game_core::test_support::fixtures::test_investigator;
use game_core::EngineOutcome;
use leptos::prelude::{document, provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::store::{reduce, ClientState};
use web::turn_tracker::TurnTrackerView;

wasm_bindgen_test_configure!(run_in_browser);

async fn mount_at(phase: Phase, round: u32) {
    game_core::test_support::install_test_registry();
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_phase(phase)
        .with_round(round)
        .build();
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <TurnTrackerView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;
}

fn last_tracker() -> web_sys::Element {
    let nodes = document()
        .query_selector_all(".turn-tracker")
        .expect("query ok");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .turn-tracker")
}

#[wasm_bindgen_test]
async fn lists_all_phases_substeps_and_round() {
    mount_at(Phase::Investigation, 2).await;
    let t = last_tracker().text_content().unwrap_or_default();
    assert!(t.contains("Round 2"), "round missing: {t}");
    assert!(t.contains("Mythos"), "Mythos missing: {t}");
    assert!(t.contains("Investigation"), "Investigation missing: {t}");
    assert!(t.contains("Enemy"), "Enemy missing: {t}");
    assert!(t.contains("Upkeep"), "Upkeep missing: {t}");
    assert!(
        t.contains("Place 1 doom on the current agenda."),
        "a Mythos sub-step missing: {t}"
    );
    assert!(t.contains("player window"), "player windows missing: {t}");
}

#[wasm_bindgen_test]
async fn current_phase_is_highlighted() {
    mount_at(Phase::Enemy, 1).await;
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
        current.contains("Enemy"),
        "current phase should be Enemy: {current}"
    );
    assert!(
        !current.contains("Mythos"),
        "only the Enemy block is current: {current}"
    );
}
