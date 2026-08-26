//! Headless tests for the investigator panel's three named controls (S6, #541):
//! End turn, Gain resource and Draw.
//!
//! Each asserts what a player can see and do — the control exists, is disabled
//! when nothing anchors to it, and submits one particular `ResolveInput` on click
//! with no intermediate menu — never how the component computed it.
//!
//! `AnchoredControl` is mounted directly rather than through `BoardView` so these
//! stay off the card registry; the whole-panel wiring is covered by
//! `tests/board_surfaces.rs`.
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::InvestigatorId;
use game_core::test_support::fixtures::awaiting_pick_single_with;
use game_core::{ChoiceOption, InputResponse, OptionId, OptionTarget, PlayerAction};
use leptos::prelude::*;
use protocol::ClientMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::controls::AnchoredControl;
use web::interaction::PendingOptions;
use web::store::ClientState;
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

const INV: InvestigatorId = InvestigatorId(1);

/// Mount one `AnchoredControl` for `target`, with the live prompt offering
/// `options`. Returns the receiver so a test can read submitted frames.
async fn mount(
    label: &str,
    class: &'static str,
    target: OptionTarget,
    options: Vec<ChoiceOption>,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    store.update(|s| s.outcome = Some(awaiting_pick_single_with("Choose an action", options)));
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    let label = label.to_string();
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(PendingOptions(pending));
        let target = target.clone();
        let label = label.clone();
        view! { <div class="pc-root"><AnchoredControl label=label class=class target=target/></div> }
    });
    leptos::task::tick().await;
    rx
}

/// The control in the LAST-mounted wrapper — scoped so DOM accumulation across
/// tests on the one page can't let an earlier mount answer for a later one.
fn control(class: &str) -> web_sys::HtmlElement {
    let roots = document().query_selector_all(".pc-root").expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<web_sys::Element>().ok())
        .expect("a .pc-root")
        .query_selector(&format!(".{class}"))
        .expect("query")
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
        .expect("the control renders")
}

fn is_disabled(el: &web_sys::HtmlElement) -> bool {
    el.dyn_ref::<web_sys::HtmlButtonElement>()
        .expect("a button")
        .disabled()
}

fn submitted(rx: &mut mpsc::UnboundedReceiver<ClientMessage>) -> InputResponse {
    match rx.try_recv().expect("a frame was sent after tick") {
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput { response },
        } => response,
        other @ ClientMessage::Submit { .. } => panic!("expected ResolveInput, got {other:?}"),
    }
}

#[wasm_bindgen_test]
async fn end_turn_renders_disabled_when_nothing_anchors_to_it() {
    // The Mythos phase: the control is still there, greyed out, so the panel does
    // not reflow and the player can see it is not their moment (#541).
    let _ = mount(
        "End turn",
        "turn-control",
        OptionTarget::TurnControl(INV),
        Vec::new(),
    )
    .await;
    let el = control("turn-control");
    assert_eq!(el.text_content().unwrap_or_default(), "End turn");
    assert!(is_disabled(&el), "no live option ⇒ disabled");
    assert!(
        !el.class_name().contains("actionable"),
        "a dead control does not glow: {}",
        el.class_name()
    );
}

#[wasm_bindgen_test]
async fn end_turn_glows_and_submits_pick_single_on_click() {
    let mut rx = mount(
        "End turn",
        "turn-control",
        OptionTarget::TurnControl(INV),
        vec![ChoiceOption::new(OptionId(3), "End turn").at(OptionTarget::TurnControl(INV))],
    )
    .await;
    let el = control("turn-control");
    assert!(!is_disabled(&el));
    assert!(el.class_name().contains("actionable"));

    // One click, no intermediate menu — the label already says what it does.
    el.click();
    leptos::task::tick().await;
    assert_eq!(submitted(&mut rx), InputResponse::PickSingle(OptionId(3)));
}

#[wasm_bindgen_test]
async fn gain_resource_submits_its_own_option() {
    let mut rx = mount(
        "Gain resource",
        "resource-control",
        OptionTarget::ResourcePool(INV),
        vec![
            ChoiceOption::new(OptionId(0), "End turn").at(OptionTarget::TurnControl(INV)),
            ChoiceOption::new(OptionId(1), "Gain resource").at(OptionTarget::ResourcePool(INV)),
            ChoiceOption::new(OptionId(2), "Draw").at(OptionTarget::PlayerDeck(INV)),
        ],
    )
    .await;
    let el = control("resource-control");
    assert!(!is_disabled(&el));
    el.click();
    leptos::task::tick().await;
    assert_eq!(
        submitted(&mut rx),
        InputResponse::PickSingle(OptionId(1)),
        "each control picks the option anchored to its own surface"
    );
}

#[wasm_bindgen_test]
async fn draw_submits_its_own_option() {
    let mut rx = mount(
        "Draw",
        "draw-control",
        OptionTarget::PlayerDeck(INV),
        vec![
            ChoiceOption::new(OptionId(0), "End turn").at(OptionTarget::TurnControl(INV)),
            ChoiceOption::new(OptionId(2), "Draw").at(OptionTarget::PlayerDeck(INV)),
        ],
    )
    .await;
    control("draw-control").click();
    leptos::task::tick().await;
    assert_eq!(submitted(&mut rx), InputResponse::PickSingle(OptionId(2)));
}

#[wasm_bindgen_test]
async fn a_control_ignores_another_investigators_option() {
    // Multiplayer: investigator 2's Draw must not light up investigator 1's deck.
    let _ =
        mount(
            "Draw",
            "draw-control",
            OptionTarget::PlayerDeck(INV),
            vec![ChoiceOption::new(OptionId(0), "Draw")
                .at(OptionTarget::PlayerDeck(InvestigatorId(2)))],
        )
        .await;
    assert!(is_disabled(&control("draw-control")));
}
