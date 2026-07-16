//! Headless render test for the Act/Agenda cards. Own binary so it installs the
//! real `cards::REGISTRY` (registry install is first-wins per process); mounts
//! `act_agenda_view` directly (no investigator panel → no `TEST_INV` lookup).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{Act, Agenda, CardCode, GameStateBuilder};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::PendingOptions;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

fn section_text() -> String {
    let nodes = document()
        .query_selector_all(".act-agenda")
        .expect("query ok");
    nodes
        .item(nodes.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .and_then(|el| el.text_content())
        .unwrap_or_default()
}

#[wasm_bindgen_test]
async fn act_and_agenda_render_name_text_and_thresholds() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Act 01109 "The Barrier" (Objective text); Agenda 01107 "They're Getting
    // Out!" (Forced text).
    let mut state = GameStateBuilder::new().build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 2,
        resolution: None,
    }];
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_doom = 1;

    leptos::mount::mount_to_body(move || web::act_agenda::act_agenda_view(&state));
    leptos::task::tick().await;

    let text = section_text();
    assert!(text.contains("The Barrier"), "act name missing: {text}");
    assert!(
        text.contains("clues to advance: 2"),
        "act threshold missing: {text}"
    );
    assert!(
        text.contains("Objective"),
        "act ability text missing: {text}"
    );
    assert!(
        text.contains("They're Getting Out!"),
        "agenda name missing: {text}"
    );
    assert!(text.contains("doom 1/3"), "agenda doom missing: {text}");
}

/// The last-mounted `.card--act` — `mount_to_body` accumulates DOM across tests in
/// this binary, so scope to the newest card (the `last_slot`/`last_root` precedent).
fn act_card() -> web_sys::Element {
    let cards = document().query_selector_all(".card--act").expect("query");
    cards
        .item(cards.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .card--act")
}

/// Mount `act_agenda_view` (act 01109) with a store carrying `outcome`, a derived
/// `PendingOptions`, an `OutboundTx`, and a capturing channel.
async fn mount_with_prompt(
    outcome: game_core::EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let mut state = GameStateBuilder::new().build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 2,
        resolution: None,
    }];
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        web::act_agenda::act_agenda_view(&state)
    });
    leptos::task::tick().await;
    rx
}

#[wasm_bindgen_test]
async fn act_card_glows_and_advances_via_menu() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Advance act",
            OptionTarget::Act,
        )],
    );
    let mut rx = mount_with_prompt(outcome).await;
    let card = act_card();
    assert!(card.class_name().contains("actionable"), "act card glows");
    card.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    let item = card
        .query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item");
    assert_eq!(item.text_content().unwrap_or_default(), "Advance act");
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
async fn act_card_inert_without_an_act_anchored_option() {
    // Option anchors Global (not Act) → the act card stays inert.
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "End turn",
            OptionTarget::Global,
        )],
    );
    let _rx = mount_with_prompt(outcome).await;
    assert!(!act_card().class_name().contains("actionable"));
}
