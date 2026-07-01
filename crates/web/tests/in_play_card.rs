//! Headless tests for `InPlayCardView` (interactivity S4, #539): a card whose
//! `CardInstance` anchor has an option glows and opens a menu that submits
//! `PickSingle`; an inert instance has no glow.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{CardCode, CardInPlay, CardInstanceId};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::card::InPlayCardView;
use web::interaction::PendingOptions;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

fn last_slot() -> web_sys::Element {
    let slots = document().query_selector_all(".card-slot").expect("query");
    slots
        .item(slots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .card-slot")
}

/// Mount `InPlayCardView` (Machete 01020, instance 3) with a store carrying
/// `outcome`, a derived `PendingOptions`, and a capturing channel.
async fn mount(outcome: game_core::EngineOutcome) -> mpsc::UnboundedReceiver<ClientMessage> {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    let inst = CardInPlay::enter_play(CardCode::new("01020"), CardInstanceId(3));
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        view! { <InPlayCardView instance=inst.clone()/> }
    });
    leptos::task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn activatable_in_play_card_opens_a_menu_and_submits() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Activate",
            OptionTarget::CardInstance(CardInstanceId(3)),
        )],
    );
    let mut rx = mount(outcome).await;
    let slot = last_slot();
    assert!(slot.class_name().contains("actionable"), "slot glows");
    slot.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    let item = slot
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Activate");
    item.click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn inert_in_play_card_has_no_glow() {
    // Option anchors to a different instance → this card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Activate",
            OptionTarget::CardInstance(CardInstanceId(99)),
        )],
    );
    let _ = mount(outcome).await;
    assert!(!last_slot().class_name().contains("actionable"));
}
