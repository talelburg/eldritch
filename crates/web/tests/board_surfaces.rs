//! Headless tests for the per-investigator board surfaces S6 adds (#541): each
//! panel carries its own player deck (card back + remaining count) and its own
//! End turn / Gain resource controls.
//!
//! Mounted through `BoardView`, so this asserts the *wiring* — that each panel
//! gets its own investigator's surfaces — where `tests/panel_controls.rs` covers
//! one control's behaviour in isolation.
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, GameStateBuilder};
use game_core::test_support::fixtures::test_investigator;
use game_core::EngineOutcome;
use leptos::prelude::*;
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `BoardView` against a fresh store carrying `state`, and return the
/// last-mounted wrapper so absence assertions are scoped to this test.
async fn mount(state: game_core::state::GameState) -> web_sys::Element {
    // Panels read investigator-card capacity from the registry (#448).
    game_core::test_support::install_test_registry();
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(web::interaction::PendingOptions(pending));
        view! { <div class="bs-root"><BoardView/></div> }
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
    let roots = document().query_selector_all(".bs-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .bs-root")
}

/// The `.deck-count` text of each `.player-deck`, in panel order.
fn deck_counts(root: &web_sys::Element) -> Vec<String> {
    let decks = root.query_selector_all(".player-deck").expect("query");
    (0..decks.length())
        .map(|i| {
            decks
                .item(i)
                .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
                .expect("Element")
                .query_selector(".deck-count")
                .expect("query")
                .and_then(|n| n.text_content())
                .expect("a count")
        })
        .collect()
}

#[wasm_bindgen_test]
async fn each_panel_shows_its_own_deck_count() {
    // Multiplayer: whose deck a count refers to must never be a guess.
    let mut one = test_investigator(1);
    one.deck = vec![CardCode::new("a"), CardCode::new("b"), CardCode::new("c")];
    let mut two = test_investigator(2);
    two.deck = vec![CardCode::new("d")];
    let state = GameStateBuilder::new()
        .with_investigator(one)
        .with_investigator(two)
        .build();

    let root = mount(state).await;
    assert_eq!(deck_counts(&root), vec!["3".to_string(), "1".to_string()]);
}

#[wasm_bindgen_test]
async fn an_empty_deck_still_renders_its_element() {
    // The worst possible moment for the board to silently lose a surface.
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    let root = mount(state).await;
    assert_eq!(deck_counts(&root), vec!["0".to_string()]);
    let deck = root
        .query_selector(".player-deck .deck-back")
        .expect("query");
    assert!(deck.is_some(), "the card back renders at zero too");
}

#[wasm_bindgen_test]
async fn the_named_controls_are_present_and_dead_with_no_prompt() {
    // No prompt is live, so nothing anchors anywhere: every control renders,
    // disabled, so the panel's shape does not depend on the phase.
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    let root = mount(state).await;
    for class in [".turn-control", ".resource-control", ".draw-control"] {
        let el = root
            .query_selector(class)
            .expect("query")
            .and_then(|n| n.dyn_into::<web_sys::HtmlButtonElement>().ok())
            .unwrap_or_else(|| panic!("`{class}` renders on the panel"));
        assert!(el.disabled(), "`{class}` is disabled with no live option");
    }
}

#[wasm_bindgen_test]
async fn the_board_carries_no_action_bar() {
    // #206's closer: the sticky bar is deleted, and a merge must not bring it back.
    let state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    let root = mount(state).await;
    assert!(
        root.query_selector(".action-bar").expect("query").is_none(),
        "no floating bar at the bottom of the screen"
    );
}
