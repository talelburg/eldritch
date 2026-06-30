//! Headless render tests for the `EnemyCard` component. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use game_core::test_support::fixtures::test_enemy;
use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::enemy_card::EnemyCard;

wasm_bindgen_test_configure!(run_in_browser);

fn last_card() -> web_sys::Element {
    let cards = leptos::prelude::document()
        .query_selector_all(".card")
        .expect("query_selector_all");
    cards
        .item(cards.length() - 1)
        .expect("at least one .card")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
}

#[wasm_bindgen_test]
async fn engaged_enemy_renders_stats_keywords_exhausted() {
    let mut e = test_enemy(1, "Ghoul Priest");
    e.fight = 4;
    e.evade = 4;
    e.max_health = 2;
    e.damage = 0;
    e.hunter = true;
    e.retaliate = true;
    e.exhausted = true;
    leptos::mount::mount_to_body(move || view! { <EnemyCard enemy=e.clone()/> });
    leptos::task::tick().await;

    let card = last_card();
    let classes = card.class_name();
    assert!(
        classes.contains("card--enemy"),
        "enemy class missing: {classes}"
    );
    assert!(
        classes.contains("card--exhausted"),
        "exhausted class missing: {classes}"
    );
    let html = card.inner_html();
    assert!(html.contains("Ghoul Priest"), "name missing: {html}");
    assert!(html.contains("fight 4"), "fight chip missing: {html}");
    assert!(html.contains("health 0/2"), "health chip missing: {html}");
    assert!(html.contains("Hunter"), "hunter chip missing: {html}");
    assert!(html.contains("Retaliate"), "retaliate chip missing: {html}");
    assert!(
        html.contains("Exhausted"),
        "exhausted badge missing: {html}"
    );
}

#[wasm_bindgen_test]
async fn ready_enemy_is_not_dimmed() {
    let e = test_enemy(2, "Swarm of Rats");
    leptos::mount::mount_to_body(move || view! { <EnemyCard enemy=e.clone()/> });
    leptos::task::tick().await;
    assert!(
        !last_card().class_name().contains("card--exhausted"),
        "ready enemy must not be dimmed"
    );
}
