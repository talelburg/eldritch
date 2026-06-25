//! Headless tests for `AwaitingInputView`'s `PickSingle` option-list branch
//! (#447): feed an `AwaitingInput` outcome carrying structured `options`
//! through the store, assert the option buttons render with their labels, and
//! that clicking one submits the matching `ResolveInput(PickSingle(id))` frame.
//!
//! The `PickMultiple` (commit/mulligan) branch is covered by `tests/input.rs`.
//! wasm32-only (browser DOM + the wasm-only transport types).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::GameStateBuilder;
use game_core::state::InvestigatorId;
use game_core::test_support::fixtures::{awaiting_pick_single_input, test_investigator};
use game_core::{InputResponse, OptionId, PlayerAction};
use leptos::prelude::*;
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::input::AwaitingInputView;
use web::store::{reduce, ClientState};
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// A minimal game with one active investigator.
fn base_game() -> game_core::state::GameState {
    GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build()
}

/// Mount `AwaitingInputView` with a fresh store + outbound channel, feed one
/// `Hello` with `state` and `outcome`, tick, and return the receiver so
/// tests can read submitted frames.
async fn mount(
    state: game_core::state::GameState,
    outcome: game_core::EngineOutcome,
) -> mpsc::UnboundedReceiver<ClientMessage> {
    let store = RwSignal::new(ClientState::default());
    let (tx, rx) = mpsc::unbounded::<ClientMessage>();
    let tx_for_mount: OutboundTx = tx;
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx_for_mount.clone());
        leptos::view! { <AwaitingInputView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome,
            },
        );
    });
    leptos::task::tick().await;
    rx
}

/// The last mounted `.awaiting-input` section (DOM accumulates across tests
/// in one browser page — same approach as `tests/input.rs`).
fn last_section() -> web_sys::Element {
    let secs = leptos::prelude::document()
        .query_selector_all(".awaiting-input")
        .expect("query");
    secs.item(secs.length() - 1)
        .expect("at least one .awaiting-input section")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
}

/// Click the nth element matching `selector` inside `section`.
fn click_in(section: &web_sys::Element, selector: &str, nth: u32) {
    let els = section.query_selector_all(selector).expect("query");
    els.item(nth)
        .expect("element present")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
}

/// Extract the `PickSingle` id from a submitted `ClientMessage::Submit`.
fn pick_single_id(frame: ClientMessage) -> u32 {
    match frame {
        ClientMessage::Submit {
            action:
                PlayerAction::ResolveInput {
                    response: InputResponse::PickSingle(OptionId(id)),
                },
        } => id,
        other @ ClientMessage::Submit { .. } => {
            panic!("expected ResolveInput/PickSingle, got {other:?}")
        }
    }
}

// ---- Tests ------------------------------------------------------------------

#[wasm_bindgen_test]
async fn pick_single_renders_prompt_and_both_option_buttons() {
    let _rx = mount(base_game(), awaiting_pick_single_input("Choose an action")).await;
    let section = last_section();
    let html = section.inner_html();
    assert!(html.contains("Choose an action"), "prompt missing: {html}");
    assert!(html.contains("End turn"), "option 0 label missing: {html}");
    assert!(
        html.contains("Investigate"),
        "option 1 label missing: {html}"
    );
    // Exactly two `.option` buttons.
    let buttons = section.query_selector_all(".option").expect("query");
    assert_eq!(
        buttons.length(),
        2,
        "expected 2 option buttons, got {}",
        buttons.length()
    );
}

#[wasm_bindgen_test]
async fn pick_single_clicking_first_option_submits_pick_single_0() {
    let mut rx = mount(base_game(), awaiting_pick_single_input("Choose an action")).await;
    let section = last_section();
    click_in(&section, ".option", 0);
    leptos::task::tick().await;
    let frame = rx
        .try_recv()
        .expect("a frame after tick — click handler must have fired");
    assert_eq!(pick_single_id(frame), 0, "expected OptionId(0)");
}

#[wasm_bindgen_test]
async fn pick_single_clicking_second_option_submits_pick_single_1() {
    let mut rx = mount(base_game(), awaiting_pick_single_input("Choose an action")).await;
    let section = last_section();
    click_in(&section, ".option", 1);
    leptos::task::tick().await;
    let frame = rx
        .try_recv()
        .expect("a frame after tick — click handler must have fired");
    assert_eq!(pick_single_id(frame), 1, "expected OptionId(1)");
}

#[wasm_bindgen_test]
async fn pick_single_does_not_render_commit_button_or_hand_list() {
    // The `.commit` and `.commit-hand` elements belong to the PickMultiple
    // branch; they must NOT appear when options are present.
    let _rx = mount(base_game(), awaiting_pick_single_input("Choose an action")).await;
    let section = last_section();
    let html = section.inner_html();
    assert!(
        !html.contains("class=\"commit\"") && !html.contains("class='commit'"),
        "commit button should not appear in PickSingle branch: {html}"
    );
    assert!(
        !html.contains("commit-hand"),
        "commit-hand list should not appear in PickSingle branch: {html}"
    );
}
