//! Headless render test for the location-card fields that come from the corpus
//! (traits / ability text / victory). Its own binary so it can install the real
//! `cards::REGISTRY` without colliding with `tests/map.rs`'s synthetic registry
//! (registry install is first-wins per process). Mounts `location_map` directly
//! (no investigator panel → no `TEST_INV` capacity lookup). wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, GameStateBuilder};
use game_core::test_support::fixtures::test_location;
use leptos::prelude::document;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// `textContent` of the last-mounted map node whose `data-loc` equals `name`.
fn node_text(name: &str) -> String {
    let maps = document().query_selector_all(".map").expect("query ok");
    let last = maps
        .item(maps.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .map section");
    last.query_selector(&format!(".map-location[data-loc=\"{name}\"]"))
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn revealed_location_shows_metadata_text_and_victory() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Attic 01113: victory 1, Forced text "After you enter the Attic: Take 1 horror."
    let mut attic = test_location(1, "Attic");
    attic.code = CardCode::new("01113");
    attic.revealed = true;
    let game = GameStateBuilder::new().with_location(attic).build();
    leptos::mount::mount_to_body(move || web::map::location_map(&game));
    leptos::task::tick().await;

    let text = node_text("Attic");
    assert!(text.contains("Victory 1"), "victory chip missing: {text}");
    assert!(text.contains("horror"), "ability text missing: {text}");
}

#[wasm_bindgen_test]
async fn revealed_location_shows_metadata_traits() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Miskatonic University 01129: traits "Arkham."
    let mut misk = test_location(2, "Miskatonic University");
    misk.code = CardCode::new("01129");
    misk.revealed = true;
    let game = GameStateBuilder::new().with_location(misk).build();
    leptos::mount::mount_to_body(move || web::map::location_map(&game));
    leptos::task::tick().await;

    let text = node_text("Miskatonic University");
    assert!(text.contains("Arkham"), "traits missing: {text}");
}

#[wasm_bindgen_test]
async fn unrevealed_location_withholds_metadata() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Attic 01113 has victory 1 + Forced text ("...Take 1 horror."); unrevealed
    // must withhold all of it (hidden info).
    let mut attic = test_location(3, "Hidden Attic");
    attic.code = CardCode::new("01113");
    attic.revealed = false;
    let game = GameStateBuilder::new().with_location(attic).build();
    leptos::mount::mount_to_body(move || web::map::location_map(&game));
    leptos::task::tick().await;

    let text = node_text("Hidden Attic");
    assert!(
        text.contains("unrevealed"),
        "unrevealed label missing: {text}"
    );
    assert!(
        !text.contains("Victory"),
        "victory must be withheld: {text}"
    );
    assert!(
        !text.contains("horror"),
        "ability text must be withheld: {text}"
    );
    assert!(!text.contains("shroud"), "shroud must be withheld: {text}");
}
