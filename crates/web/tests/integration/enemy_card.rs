//! Headless render tests for the `EnemyCard` tile: its name, stats and marks on
//! the tile, its whole card in the hover popover. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use cards::TEST_REGISTRIES;
use futures::channel::mpsc::{self, UnboundedReceiver};
use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{ChoiceOption, EngineOutcome, OptionId, OptionTarget};
use game_core::state::{Enemy, EnemyId};
use game_core::test_support::{self, enemy};
use leptos::prelude::*;
use leptos::task;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::enemy_card::EnemyCard;
use web::interaction::{self, PendingOptions};
use web::registry::provide_cards;
use web::store::ClientState;
use web::transport::OutboundTx;
use web_sys::{Element, HtmlElement};

use crate::common;

/// The last-mounted enemy tile.
fn last_card() -> Element {
    common::last_mounted(".tile.card--enemy")
}

#[wasm_bindgen_test]
async fn engaged_enemy_renders_stats_keywords_exhausted() {
    let e = enemy(1)
        .name("Test Brute")
        .fight(4)
        .evade(4)
        .health(2)
        .hunter()
        .retaliate()
        .exhausted()
        .mint(TEST_REGISTRIES, 1);
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <EnemyCard enemy=e.clone()/> }
    });
    task::tick().await;

    let card = last_card();
    let classes = card.class_name();
    assert!(
        classes.contains("card--exhausted"),
        "exhausted class missing: {classes}"
    );
    let tile = card.text_content().unwrap_or_default();
    assert!(tile.contains("Test Brute"), "name missing: {tile}");
    assert!(
        tile.contains("4\u{b7}2\u{b7}4"),
        "fight, health, evade: {tile}"
    );
    assert!(tile.contains("Exhausted"), "exhausted mark missing: {tile}");

    // The whole card, with every stat and keyword, is in the popover.
    common::hover(&card).await;
    let html = card
        .query_selector(".popover")
        .expect("query")
        .expect("hovering opens the popover")
        .inner_html();
    common::unhover(&card).await;
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
async fn enemy_shows_its_printed_traits() {
    // Ghoul Minion 01160: traits "Humanoid. Monster. Ghoul."
    let e = enemy(3).as_card("01160").mint(TEST_REGISTRIES, 1);
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <EnemyCard enemy=e.clone()/> }
    });
    task::tick().await;

    let card = last_card();
    common::hover(&card).await;
    let traits = card
        .query_selector(".popover .card-traits")
        .expect("query ok")
        .and_then(|el| el.text_content())
        .unwrap_or_default();
    common::unhover(&card).await;
    assert_eq!(traits, "Humanoid. Monster. Ghoul.");
}

#[wasm_bindgen_test]
async fn ready_enemy_is_not_dimmed() {
    let e = enemy(2).mint(TEST_REGISTRIES, 1);
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <EnemyCard enemy=e.clone()/> }
    });
    task::tick().await;
    assert!(
        !last_card().class_name().contains("card--exhausted"),
        "ready enemy must not be dimmed"
    );
}

/// Mount an `EnemyCard` with a store-derived `PendingOptions` signal + an
/// outbound channel, set the store's `outcome` directly (`pending_options` reads
/// `outcome`, not `game`, so no `GameState` is needed), and return the
/// submitted-frame receiver.
async fn mount_enemy(enemy: Enemy, outcome: EngineOutcome) -> UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    // Set the prompt before mount: EnemyCard reads `pending` once at setup (in the
    // app it is re-created inside BoardView's reactive scope on each store change),
    // so the outcome must be live when the card mounts.
    store.update(|s| {
        s.outcome = Some(outcome);
        common::you_hold_every_investigator(s);
    });
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(interaction::pending_options));
        provide_context(PendingOptions(pending));
        view! { <EnemyCard enemy=enemy.clone()/> }
    });
    task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn actionable_enemy_glows_opens_menu_and_submits() {
    let e = enemy(7).mint(TEST_REGISTRIES, 1);
    let outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Fight").at(OptionTarget::Enemy(EnemyId(7)))],
    );
    let mut rx = mount_enemy(e, outcome).await;

    let card = last_card();
    assert!(card.class_name().contains("actionable"), "enemy card glows");

    card.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a .menu-hit layer")
        .click();
    task::tick().await;

    let item = card
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Fight");
    item.click();
    task::tick().await;

    let msg = rx.try_recv().expect("a frame was sent after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response, .. },
            ..
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn enemy_without_a_matching_option_is_inert() {
    let e = enemy(7).mint(TEST_REGISTRIES, 1);
    // Option anchors to a different enemy → this card stays inert.
    let outcome = test_support::awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(OptionId(0), "Fight").at(OptionTarget::Enemy(EnemyId(8)))],
    );
    let _ = mount_enemy(e, outcome).await;
    assert!(!last_card().class_name().contains("actionable"));
}
