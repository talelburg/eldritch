//! Headless render tests for the spatial board map (#497). Mount `BoardView`,
//! feed a constructed `GameState`, assert on the rendered DOM. wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::state::{GameStateBuilder, LocationId};
use game_core::test_support::fixtures::{test_enemy, test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{document, provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `BoardView`, feed one `Hello` carrying `state`, tick, return nothing —
/// assertions query the live DOM via `document()`.
async fn mount_state(state: game_core::state::GameState) {
    game_core::test_support::install_test_registry();
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
}

/// `textContent` of the map node whose `data-loc` equals `loc_name`, scoped to
/// the LAST mounted `<section class="map">` so that DOM accumulation across tests
/// does not make an earlier test's node shadow this one (same pattern as the
/// `board.rs` wasm tests).
fn node_text(loc_name: &str) -> String {
    let maps = document()
        .query_selector_all(".map")
        .expect("query_selector_all ok");
    let len = maps.length();
    if len == 0 {
        return String::new();
    }
    let last_map = maps
        .item(len - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("last .map is an Element");
    let sel = format!(".map-location[data-loc=\"{loc_name}\"]");
    last_map
        .query_selector(&sel)
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn investigator_renders_inside_its_location_node() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(inv)
        .build();
    mount_state(state).await;
    assert!(
        node_text("Study").contains("Test Investigator 1"),
        "investigator token must render inside its location node; Study node = {:?}",
        node_text("Study"),
    );
}

#[wasm_bindgen_test]
async fn unengaged_enemy_renders_inside_its_location_node() {
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = None;
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(test_investigator(1))
        .with_enemy(enemy)
        .build();
    mount_state(state).await;
    assert!(
        node_text("Study").contains("Mock Ghoul"),
        "unengaged enemy must render inside its location node; Study node = {:?}",
        node_text("Study"),
    );
}
