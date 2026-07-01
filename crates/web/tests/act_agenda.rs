//! Headless render test for the Act/Agenda cards. Own binary so it installs the
//! real `cards::REGISTRY` (registry install is first-wins per process); mounts
//! `act_agenda_view` directly (no investigator panel → no `TEST_INV` lookup).
#![cfg(target_arch = "wasm32")]

use game_core::state::{Act, Agenda, CardCode, GameStateBuilder};
use leptos::prelude::document;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn section_text() -> String {
    let nodes = document()
        .query_selector_all(".act-agenda")
        .expect("query ok");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn act_and_agenda_render_name_text_and_thresholds() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Act 01109 "The Barrier" (Objective text); Agenda 01107 "They're Getting
    // Out!" (Forced text).
    let mut state = GameStateBuilder::new().build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_doom = 1;

    leptos::mount::mount_to_body(move || web::act_agenda::act_agenda_view(&state));
    leptos::task::tick().await;

    let text = section_text();
    assert!(text.contains("The Barrier"), "act name missing: {text}");
    assert!(
        text.contains("clues to advance: 2"),
        "act threshold missing: {text}"
    );
    assert!(
        text.contains("Objective"),
        "act ability text missing: {text}"
    );
    assert!(
        text.contains("They're Getting Out!"),
        "agenda name missing: {text}"
    );
    assert!(text.contains("doom 1/3"), "agenda doom missing: {text}");
}
