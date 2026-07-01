//! Headless render tests for the `Card` component. wasm32-only (browser DOM).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId};
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use std::collections::BTreeSet;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::card::{Card, HandCardView};
use web::interaction::{MultiSelect, PendingOptions};
use web::store::ClientState;
use web::transport::OutboundTx;

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
async fn treachery_renders_generic_face_with_clues() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    // Cover Up 01007: treachery/weakness, traits "Task.", Revelation text;
    // enters the threat area with clues on the card.
    let mut inst = CardInPlay::enter_play(CardCode::new("01007"), CardInstanceId(0));
    inst.clues = 3;
    leptos::mount::mount_to_body(
        move || view! { <Card code=CardCode::new("01007") in_play=inst.clone()/> },
    );
    leptos::task::tick().await;

    assert!(
        last_card_classes().contains("card--generic"),
        "treachery should use the generic arm"
    );
    let html = last_card_html();
    assert!(html.contains("Cover Up"), "name missing: {html}");
    assert!(html.contains("Task"), "trait missing: {html}");
    assert!(
        html.contains("clues 3"),
        "clues-on-card chip missing: {html}"
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
    assert!(
        !last_card_html().contains("card-cost"),
        "in-play card must not show a cost corner regardless of exhaustion"
    );
}

/// The last-mounted `.hand-slot`.
fn last_slot() -> web_sys::Element {
    let slots = leptos::prelude::document()
        .query_selector_all(".hand-slot")
        .expect("query");
    slots
        .item(slots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .hand-slot")
}

/// Mount a `HandCardView` (Machete 01020, investigator 1, index 0) with a store
/// carrying `outcome`, `PendingOptions` derived from it, a `MultiSelect` whose
/// `active` reflects the outcome, and a capturing outbound channel.
async fn mount_hand(
    outcome: game_core::EngineOutcome,
) -> (
    RwSignal<BTreeSet<u32>>,
    mpsc::UnboundedReceiver<ClientMessage>,
) {
    let _ = game_core::card_registry::install(cards::REGISTRY);
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let selected = RwSignal::new(BTreeSet::<u32>::new());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(MultiSelect { active, selected });
        view! {
            <HandCardView code=CardCode::new("01020") investigator=InvestigatorId(1) index=0/>
        }
    });
    leptos::task::tick().await;
    (selected, rx)
}

#[wasm_bindgen_test]
async fn playable_hand_card_opens_a_play_menu_and_submits() {
    let outcome = awaiting_pick_single_with(
        "Choose an action",
        vec![ChoiceOption::new(
            OptionId(0),
            "Play Machete",
            OptionTarget::HandCard {
                investigator: InvestigatorId(1),
                hand_index: 0,
            },
        )],
    );
    let (_selected, mut rx) = mount_hand(outcome).await;

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
    assert_eq!(item.text_content().unwrap_or_default(), "Play Machete");
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
async fn multi_select_active_makes_hand_card_toggle_selected() {
    let (selected, _rx) = mount_hand(game_core::test_support::fixtures::awaiting_commit_input(
        "Commit cards",
    ))
    .await;
    let slot = last_slot();
    assert!(
        !slot.class_name().contains("actionable"),
        "no Play menu in select mode"
    );
    slot.clone()
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;
    assert!(selected.get_untracked().contains(&0), "index 0 selected");
    assert!(
        last_slot().class_name().contains("selected"),
        "selected ring shown"
    );
}

#[wasm_bindgen_test]
async fn hand_card_glows_for_a_reaction_anchored_by_code() {
    // A HandCardByCode-anchored option (a Fast reaction event) glows the hand
    // card of that code (Machete 01020 as a stand-in) and opens its menu.
    let outcome = awaiting_pick_single_with(
        "You may play a card",
        vec![ChoiceOption::new(
            OptionId(0),
            "Play Machete from hand",
            OptionTarget::HandCardByCode {
                investigator: InvestigatorId(1),
                code: CardCode::new("01020"),
            },
        )],
    );
    let (_selected, mut rx) = mount_hand(outcome).await;
    let slot = last_slot();
    assert!(
        slot.class_name().contains("actionable"),
        "reaction card glows by code"
    );
    slot.query_selector(".menu-hit")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a .menu-hit")
        .click();
    leptos::task::tick().await;
    slot.query_selector(".context-menu .menu-item")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("a menu item")
        .click();
    leptos::task::tick().await;
    let msg = rx.try_recv().expect("a frame after tick");
    match msg {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::PickSingle(OptionId(0))),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}
