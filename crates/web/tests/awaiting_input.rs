//! Headless tests for `AwaitingInputView`'s `PickSingle` option-list branch
//! (#447): feed an `AwaitingInput` outcome carrying structured `options`
//! through the store, assert the option buttons render with their labels, and
//! that clicking one submits the matching `ResolveInput(PickSingle(id))` frame.
//!
//! `PickMultiple` (commit/mulligan/discard) moved off this view in #538 — its
//! coverage is `tests/prompt_banner.rs` (Confirm/Pass) + `tests/card.rs`
//! (hand-card selection). wasm32-only (browser DOM + the wasm-only transport types).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::GameStateBuilder;
use game_core::state::InvestigatorId;
use game_core::test_support::fixtures::{
    awaiting_confirm_input, awaiting_pick_single_input, test_investigator,
};
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
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx.clone());
        leptos::view! { <AwaitingInputView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome,
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;
    rx
}

/// The last mounted `.awaiting-input` section (DOM accumulates across tests
/// in one browser page — scope to the latest subtree).
fn last_section() -> web_sys::Element {
    let secs = leptos::prelude::document()
        .query_selector_all(".awaiting-input")
        .expect("query");
    secs.item(secs.length() - 1)
        .expect("at least one .awaiting-input section")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
}

/// The trimmed text content of the nth node in a `NodeList`.
fn button_text(nodes: &web_sys::NodeList, nth: u32) -> String {
    nodes
        .item(nth)
        .expect("node present")
        .text_content()
        .unwrap_or_default()
        .trim()
        .to_owned()
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
        // `Submit` is the only `ClientMessage` variant, so this arm catches a
        // `Submit` carrying some *other* action than `ResolveInput/PickSingle`.
        // Written as `@ Submit { .. }` (not a bare `_`) because
        // clippy::match_wildcard_for_single_variants requires it.
        other @ ClientMessage::Submit { .. } => {
            panic!("expected ResolveInput/PickSingle, got {other:?}")
        }
    }
}

/// True if the frame is `ResolveInput(Confirm)`.
fn is_confirm(frame: &ClientMessage) -> bool {
    matches!(
        frame,
        ClientMessage::Submit {
            action: PlayerAction::ResolveInput {
                response: InputResponse::Confirm
            },
        }
    )
}

// ---- Tests ------------------------------------------------------------------

#[wasm_bindgen_test]
async fn pick_single_renders_prompt_and_both_option_buttons() {
    let _rx = mount(base_game(), awaiting_pick_single_input("Choose an action")).await;
    let section = last_section();

    // Prompt text via the `.prompt` element, not a raw HTML substring.
    let prompt = section
        .query_selector(".prompt")
        .expect("query")
        .expect(".prompt element present");
    assert_eq!(
        prompt.text_content().unwrap_or_default(),
        "Choose an action"
    );

    // Exactly two `.option` buttons, in order, with the expected labels.
    let buttons = section.query_selector_all(".option").expect("query");
    assert_eq!(
        buttons.length(),
        2,
        "expected 2 option buttons, got {}",
        buttons.length()
    );
    assert_eq!(
        button_text(&buttons, 0),
        "End turn",
        "option 0 label mismatch"
    );
    assert_eq!(
        button_text(&buttons, 1),
        "Investigate",
        "option 1 label mismatch"
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
async fn confirm_renders_confirm_button_and_submits_confirm() {
    let mut rx = mount(
        base_game(),
        awaiting_confirm_input("Draw an encounter card"),
    )
    .await;
    let section = last_section();

    let confirm = section.query_selector(".confirm").expect("query");
    assert!(
        confirm.is_some(),
        "Confirm prompt must render a .confirm button"
    );
    // No hand-commit UI for a Confirm prompt.
    assert!(section
        .query_selector(".commit-hand")
        .expect("query")
        .is_none());

    click_in(&section, ".confirm", 0);
    leptos::task::tick().await;
    let frame = rx.try_recv().expect("a frame after clicking Confirm");
    assert!(
        is_confirm(&frame),
        "expected ResolveInput(Confirm), got {frame:?}"
    );
}

// The bar no longer renders a Skip control — a skippable window's Pass moved to
// the bottom prompt banner (#539). That behavior is covered by
// `tests/prompt_banner.rs` (`skippable_window_shows_prompt_and_pass_submits_skip`,
// `non_skippable_pick_single_renders_no_banner`).

#[wasm_bindgen_test]
async fn pick_single_does_not_render_commit_button_or_hand_list() {
    // The `.commit` and `.commit-hand` elements belong to the PickMultiple
    // branch; they must NOT appear when options are present. DOM queries are
    // robust against compound classes / attribute-order changes.
    let _rx = mount(base_game(), awaiting_pick_single_input("Choose an action")).await;
    let section = last_section();
    assert!(
        section.query_selector(".commit").expect("query").is_none(),
        "commit button should not appear in PickSingle branch"
    );
    assert!(
        section
            .query_selector(".commit-hand")
            .expect("query")
            .is_none(),
        "commit-hand list should not appear in PickSingle branch"
    );
}

#[wasm_bindgen_test]
async fn picking_an_option_sets_the_pending_log_header() {
    let store = RwSignal::new(ClientState::default());
    let (tx, _rx) = mpsc::unbounded::<ClientMessage>();
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        provide_context::<OutboundTx>(tx.clone());
        leptos::view! { <AwaitingInputView/> }
    });
    // A PickSingle prompt whose first option has a known label.
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(base_game()),
                outcome: awaiting_pick_single_input("Choose an action"),
                events: Vec::new(),
            },
        );
    });
    leptos::task::tick().await;

    // Click the first rendered option button.
    let section = last_section();
    let buttons = section.query_selector_all(".option").expect("options");
    buttons
        .item(0)
        .expect("an option button")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
    leptos::task::tick().await;

    let header = store.with_untracked(|s| s.pending_label.clone());
    assert_eq!(
        header.as_deref(),
        Some("End turn"),
        "clicking an option must set the event-log header to the chosen option's label"
    );
}
