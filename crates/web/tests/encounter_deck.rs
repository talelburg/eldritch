//! Headless tests for the encounter-deck element (S6, #541): it shows the
//! remaining count, and its Draw button is live exactly while the Mythos step-1.4
//! prompt is up — which is a **request** anchor, since that prompt is an
//! option-less `Confirm` (ADR 0011).
//!
//! The collision this guards is the one the retired action bar hid: the encounter
//! draw and the cosmetic skill-test acknowledge are the same shape on the wire
//! but for that anchor, and the bar rendered both as an identical "Confirm".
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::{CardCode, GameStateBuilder};
use game_core::test_support::fixtures::{awaiting_confirm_input, test_investigator};
use game_core::{EngineOutcome, InputResponse, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::interaction::ConfirmAnchor;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// A `Confirm` prompt anchored to the encounter deck — the Mythos draw.
fn encounter_draw_prompt() -> EngineOutcome {
    let mut outcome = awaiting_confirm_input("Mythos step 1.4: draws an encounter card.");
    if let EngineOutcome::AwaitingInput { request, .. } = &mut outcome {
        request.target = Some(OptionTarget::EncounterDeck);
    }
    outcome
}

/// Mount the encounter-deck element for a game whose encounter deck holds
/// `deck_size` cards, with `outcome` live.
async fn mount(deck_size: usize, outcome: EngineOutcome) -> mpsc::UnboundedReceiver<ClientMessage> {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    state.encounter_deck = (0..deck_size)
        .map(|i| CardCode::new(format!("_enc{i}")))
        .collect();
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(outcome));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let anchor = Signal::derive(move || store.with(web::interaction::confirm_anchor));
        provide_context(ConfirmAnchor(anchor));
        view! { <div class="ed-root">{web::controls::encounter_deck_view(&state)}</div> }
    });
    leptos::task::tick().await;
    rx
}

fn last_root() -> web_sys::Element {
    let roots = document().query_selector_all(".ed-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("an .ed-root")
}

fn draw_button() -> web_sys::HtmlElement {
    last_root()
        .query_selector(".encounter-draw")
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("the encounter deck's Draw button renders")
}

fn is_disabled(el: &web_sys::HtmlElement) -> bool {
    el.dyn_ref::<web_sys::HtmlButtonElement>()
        .expect("a button")
        .disabled()
}

#[wasm_bindgen_test]
async fn shows_the_remaining_count() {
    let _ = mount(7, EngineOutcome::Done).await;
    let count = last_root()
        .query_selector(".deck-count")
        .expect("query")
        .and_then(|n| n.text_content())
        .expect("a count");
    assert_eq!(count, "7");
}

#[wasm_bindgen_test]
async fn renders_at_zero_without_glowing() {
    // An empty deck must not make the element vanish — the board losing a surface
    // at the worst moment is exactly what the always-present rule prevents.
    let _ = mount(0, EngineOutcome::Done).await;
    assert_eq!(
        last_root()
            .query_selector(".deck-count")
            .expect("query")
            .and_then(|n| n.text_content())
            .expect("a count"),
        "0"
    );
    let el = draw_button();
    assert!(is_disabled(&el));
    assert!(!el.class_name().contains("actionable"));
}

#[wasm_bindgen_test]
async fn draw_is_dark_for_the_unanchored_acknowledge_confirm() {
    // Same kind, same empty options, different prose — only the anchor separates
    // them, and this is the assertion that says so.
    let _ = mount(
        5,
        awaiting_confirm_input("Acknowledge the skill-test result."),
    )
    .await;
    assert!(
        is_disabled(&draw_button()),
        "the acknowledge pause is un-anchored and must not light the encounter deck"
    );
}

#[wasm_bindgen_test]
async fn draw_glows_and_submits_confirm_for_the_mythos_draw() {
    let mut rx = mount(5, encounter_draw_prompt()).await;
    let el = draw_button();
    assert!(!is_disabled(&el));
    assert!(el.class_name().contains("actionable"));

    el.click();
    leptos::task::tick().await;
    match rx.try_recv().expect("a frame was sent after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => assert_eq!(response, InputResponse::Confirm),
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}
