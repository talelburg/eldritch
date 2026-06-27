//! Headless render tests for the spatial board map (#497). Mount `BoardView`,
//! feed a constructed `GameState`, assert on the rendered DOM. wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::state::{GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::fixtures::{test_enemy, test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{document, provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};
use web_sys::Element;

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

/// Count of `<line class="map-line">` elements in the LAST mounted `.map`
/// section. Scoped to the last section so that DOM accumulation from earlier
/// test mounts does not carry over stale lines into a fresh assertion.
fn line_count() -> u32 {
    let maps = document().query_selector_all(".map").expect("query ok");
    let len = maps.length();
    if len == 0 {
        return 0;
    }
    let last_map = maps
        .item(len - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("last .map is an Element");
    last_map
        .query_selector_all("line.map-line")
        .expect("query ok")
        .length()
}

#[wasm_bindgen_test]
async fn connected_locations_draw_a_line() {
    let mut state = GameStateBuilder::new()
        .with_location(test_location(10, "Hallway"))
        .with_location(test_location(11, "Attic"))
        .with_investigator(test_investigator(1))
        .build();
    state.connect(LocationId(10), LocationId(11));
    mount_state(state).await;
    assert_eq!(
        line_count(),
        1,
        "a connected pair must draw exactly one line"
    );
}

#[wasm_bindgen_test]
async fn isolated_location_draws_no_line() {
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study")) // no connections
        .with_investigator(test_investigator(1))
        .build();
    mount_state(state).await;
    assert_eq!(line_count(), 0, "an isolated location draws no lines");
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
async fn engaged_enemy_renders_in_detail_panel_not_in_node() {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.current_location = Some(LocationId(10));
    enemy.engaged_with = Some(InvestigatorId(1));
    let state = GameStateBuilder::new()
        .with_location(test_location(10, "Study"))
        .with_investigator(inv)
        .with_enemy(enemy)
        .build();
    mount_state(state).await;

    // Not in the location node (engaged enemies leave the location box)…
    assert!(
        !node_text("Study").contains("Mock Ghoul"),
        "engaged enemy must NOT render in the location node; node = {:?}",
        node_text("Study"),
    );
    // …but present in the investigator detail panel (query last to avoid DOM accumulation).
    let investigators = document()
        .query_selector_all(".investigators")
        .expect("query_selector_all ok");
    let len = investigators.length();
    let panel = investigators
        .item(len.saturating_sub(1))
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.text_content())
        .unwrap_or_default();
    assert!(
        panel.contains("Mock Ghoul"),
        "engaged enemy must render in the detail panel; panel = {panel:?}",
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
