//! Headless render tests for `BoardView` (P6.5). Feed a constructed
//! `GameState` through the store and assert on the rendered DOM.
//! wasm32-only (browser DOM); native jobs skip this file.
#![cfg(target_arch = "wasm32")]

use game_core::state::{Act, Agenda, Phase};
use game_core::test_support::builder::TestGame;
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

fn body_html() -> String {
    leptos::prelude::document()
        .body()
        .expect("body")
        .inner_html()
}

/// Mount `BoardView` against a fresh store and feed it one `Hello`
/// carrying `state`. Ticks once so CSR effects flush, then returns the
/// rendered body HTML.
async fn render_state(state: game_core::state::GameState) -> String {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;
    body_html()
}

#[wasm_bindgen_test]
async fn phase_bar_renders_phase_round_act_agenda() {
    let mut state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_phase(Phase::Investigation)
        .with_round(3)
        .build();
    state.act_deck = vec![Act {
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        doom_threshold: 5,
        resolution: None,
    }];
    state.agenda_doom = 1;

    let html = render_state(state).await;

    assert!(html.contains("Investigation"), "phase missing: {html}");
    assert!(
        html.contains("round 3") || html.contains("Round 3"),
        "round missing: {html}"
    );
    assert!(
        html.contains("doom 1/5") || html.contains("1/5"),
        "agenda doom missing: {html}"
    );
    assert!(
        html.contains("clues 0/2") || html.contains("0/2"),
        "act threshold missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn locations_panel_renders_name_shroud_clues_revealed() {
    let mut loc = test_location(7, "Rivertown");
    loc.shroud = 3;
    loc.clues = 2;
    let state = TestGame::new()
        .with_investigator(test_investigator(1))
        .with_location(loc)
        .build();

    let html = render_state(state).await;

    assert!(html.contains("Rivertown"), "location name missing: {html}");
    assert!(html.contains("shroud 3"), "shroud missing: {html}");
    assert!(html.contains("clues 2"), "location clues missing: {html}");
    assert!(html.contains("revealed"), "revealed flag missing: {html}");
}
