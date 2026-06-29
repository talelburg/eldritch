//! Headless render tests for the `Card` component. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::state::CardCode;
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::card::Card;

wasm_bindgen_test_configure!(run_in_browser);

/// Inner HTML of the last mounted `.card` (DOM accumulates across tests on the
/// shared page — scope to the latest subtree).
fn last_card_html() -> String {
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_ref::<web_sys::Element>()
        .expect("Element")
        .inner_html()
}

async fn mount_card(code: &str) -> String {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let code = CardCode::new(code);
    leptos::mount::mount_to_body(move || view! { <Card code=code.clone()/> });
    leptos::task::tick().await;
    last_card_html()
}

#[wasm_bindgen_test]
async fn asset_renders_cost_name_traits_text_icons() {
    // Machete 01020: Guardian, cost 3, Hand slot, 1 combat icon, text with
    // [action], <b>Fight.</b>, and [combat].
    let html = mount_card("01020").await;
    assert!(html.contains("Machete"), "name missing: {html}");
    assert!(html.contains('3'), "cost missing: {html}");
    assert!(html.contains("Weapon"), "traits missing: {html}");
    assert!(html.contains("Fight."), "bold text missing: {html}");
    // [combat] / [action] become chips; assert the chip class is present.
    assert!(html.contains("chip--combat"), "combat chip missing: {html}");
}

#[wasm_bindgen_test]
async fn guardian_card_carries_class_modifier() {
    let _ = mount_card("01020").await;
    let cards = leptos::prelude::document()
        .query_selector_all(".card--guardian")
        .expect("query_selector_all");
    assert!(cards.length() >= 1, "guardian class modifier missing");
}

#[wasm_bindgen_test]
async fn unknown_code_falls_back_to_raw_code() {
    let html = mount_card("99999").await;
    assert!(html.contains("99999"), "raw code fallback missing: {html}");
}
