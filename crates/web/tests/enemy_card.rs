//! Headless render tests for the `EnemyCard` component. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::EnemyId;
use game_core::test_support::fixtures::{awaiting_pick_single_with, test_enemy};
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::enemy_card::EnemyCard;
use web::interaction::{pending_options, PendingOptions};
use web::store::ClientState;
use web::transport::OutboundTx;

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
    assert!(html.contains("evade 4"), "evade chip missing: {html}");
    assert!(
        html.contains("attack: 1 dmg · 0 hor"),
        "attack chip missing: {html}"
    );
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

/// Mount an `EnemyCard` with a store-derived `PendingOptions` signal + an
/// outbound channel, set the store's `outcome` directly (`pending_options` reads
/// `outcome`, not `game`, so no `GameState` is needed), and return the
/// submitted-frame receiver.
async fn mount_enemy(
    enemy: game_core::state::Enemy,
    outcome: game_core::EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    // Set the prompt before mount: EnemyCard reads `pending` once at setup (in the
    // app it is re-created inside BoardView's reactive scope on each store change),
    // so the outcome must be live when the card mounts.
    store.update(|s| s.outcome = Some(outcome));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(pending_options));
        provide_context(PendingOptions(pending));
        view! { <EnemyCard enemy=enemy.clone()/> }
    });
    leptos::task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn actionable_enemy_glows_opens_menu_and_submits() {
    let e = test_enemy(7, "Ghoul");
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Fight",
            OptionTarget::Enemy(EnemyId(7)),
        )],
    );
    let mut rx = mount_enemy(e, outcome).await;

    let card = last_card();
    assert!(card.class_name().contains("actionable"), "enemy card glows");

    card.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit layer")
        .click();
    leptos::task::tick().await;

    let item = card
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Fight");
    item.click();
    leptos::task::tick().await;

    let msg = rx.try_recv().expect("a frame was sent after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn enemy_without_a_matching_option_is_inert() {
    let e = test_enemy(7, "Ghoul");
    // Option anchors to a different enemy → this card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Fight",
            OptionTarget::Enemy(EnemyId(8)),
        )],
    );
    let _ = mount_enemy(e, outcome).await;
    assert!(!last_card().class_name().contains("actionable"));
}
