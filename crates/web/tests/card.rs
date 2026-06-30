//! Headless render tests for the `Card` component. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, CardInPlay, CardInstanceId};
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
    // `chip--action` can only come from the card TEXT ([action]), not a footer
    // skill chip — so it isolates the render_segments symbol→chip path.
    assert!(
        html.contains("chip--action"),
        "action chip (from text) missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn guardian_card_carries_class_modifier() {
    let _ = mount_card("01020").await;
    // Scope to the last mounted .card (DOM accumulates across tests on the
    // shared page) and assert IT carries the guardian class modifier.
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    let last = cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_into::<web_sys::Element>()
        .expect("Element");
    assert!(
        last.class_list().contains("card--guardian"),
        "last mounted card should carry the guardian class modifier"
    );
}

#[wasm_bindgen_test]
async fn unknown_code_falls_back_to_raw_code() {
    let html = mount_card("99999").await;
    assert!(html.contains("99999"), "raw code fallback missing: {html}");
}

/// Class list of the last mounted `.card` element.
fn last_card_classes() -> String {
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
        .class_name()
}

#[wasm_bindgen_test]
async fn in_play_exhausted_asset_dims_badges_and_shows_soak() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Beat Cop 01018: ally asset, health 2 / sanity 2.
    let mut inst = CardInPlay::enter_play(CardCode::new("01018"), CardInstanceId(0));
    inst.exhausted = true;
    inst.accumulated_damage = 1;
    leptos::mount::mount_to_body(
        move || view! { <Card code=CardCode::new("01018") in_play=inst.clone()/> },
    );
    leptos::task::tick().await;

    assert!(
        last_card_classes().contains("card--exhausted"),
        "exhausted class missing"
    );
    let html = last_card_html();
    assert!(
        html.contains("Exhausted"),
        "exhausted badge missing: {html}"
    );
    assert!(html.contains("dmg 1/2"), "soak chip missing: {html}");
    assert!(
        !html.contains("card-cost"),
        "in-play card must not show a cost corner: {html}"
    );
}

#[wasm_bindgen_test]
async fn in_play_ready_asset_is_not_dimmed() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let inst = CardInPlay::enter_play(CardCode::new("01018"), CardInstanceId(0));
    leptos::mount::mount_to_body(
        move || view! { <Card code=CardCode::new("01018") in_play=inst.clone()/> },
    );
    leptos::task::tick().await;
    assert!(
        !last_card_classes().contains("card--exhausted"),
        "ready card must not be dimmed"
    );
}
