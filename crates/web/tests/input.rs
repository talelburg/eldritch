//! Headless tests for `AwaitingInputView` (P6.6): feed an `AwaitingInput`
//! outcome through the store, assert the prompt + per-hand-card controls
//! render, and that committing submits the matching `ResolveInput` frame.
//! wasm32-only (browser DOM + the wasm-only transport types).
#![cfg(target_arch = "wasm32")]

use futures::channel::mpsc;
use game_core::state::GameStateBuilder;
use game_core::state::{CardCode, InvestigatorId};
use game_core::test_support::fixtures::{awaiting_commit_input, test_investigator};
use game_core::{InputResponse, PlayerAction};
use leptos::prelude::*;
use protocol::{ClientMessage, ServerMessage};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::input::AwaitingInputView;
use web::store::{reduce, ClientState};
use web::transport::OutboundTx;

wasm_bindgen_test_configure!(run_in_browser);

/// A two-card hand for investigator 1, set active.
fn two_card_game() -> game_core::state::GameState {
    let mut inv = test_investigator(1);
    inv.hand = vec![
        CardCode::new("_synth_event_a"),
        CardCode::new("_synth_event_b"),
    ];
    GameStateBuilder::new()
        .with_investigator(inv)
        .with_active_investigator(InvestigatorId(1))
        .build()
}

/// Mount `AwaitingInputView` with a fresh store + outbound channel, feed
/// one `Hello` carrying `state` and the commit prompt, tick, and return
/// the receiver so the test can read submitted frames.
async fn mount(state: game_core::state::GameState) -> mpsc::UnboundedReceiver<ClientMessage> {
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
                outcome: awaiting_commit_input("Commit cards for the skill test"),
            },
        );
    });
    leptos::task::tick().await;
    rx
}

/// The last mounted `.awaiting-input` section (DOM accumulates across
/// tests in one page — see board.rs).
fn last_section() -> web_sys::Element {
    let secs = leptos::prelude::document()
        .query_selector_all(".awaiting-input")
        .expect("query");
    secs.item(secs.length() - 1)
        .expect("at least one section")
        .dyn_into::<web_sys::Element>()
        .expect("Element")
}

fn click_in(section: &web_sys::Element, selector: &str, nth: u32) {
    let els = section.query_selector_all(selector).expect("query");
    els.item(nth)
        .expect("element present")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement")
        .click();
}

fn commit_indices(frame: ClientMessage) -> Vec<u32> {
    match frame {
        ClientMessage::Submit {
            action:
                PlayerAction::ResolveInput {
                    response: InputResponse::PickMultiple { selected },
                },
        } => selected.into_iter().map(|o| o.0).collect(),
        other @ ClientMessage::Submit { .. } => {
            panic!("expected ResolveInput/PickMultiple, got {other:?}")
        }
    }
}

#[wasm_bindgen_test]
async fn pick_multiple_button_reads_confirm() {
    // #469: the button is a neutral "Confirm" (not "Commit") for every
    // PickMultiple context; the prompt carries the per-context meaning.
    let _rx = mount(two_card_game()).await;
    let section = last_section();
    let btn = section
        .query_selector(".commit")
        .expect("query")
        .expect(".commit button present");
    assert_eq!(btn.text_content().unwrap_or_default().trim(), "Confirm");
}

#[wasm_bindgen_test]
async fn renders_prompt_and_hand_cards() {
    let _rx = mount(two_card_game()).await;
    let section = last_section();
    let html = section.inner_html();
    assert!(
        html.contains("Commit cards for the skill test"),
        "prompt missing: {html}"
    );
    assert!(html.contains("_synth_event_a"), "card a missing: {html}");
    assert!(html.contains("_synth_event_b"), "card b missing: {html}");
}

#[wasm_bindgen_test]
async fn commit_with_no_selection_submits_empty() {
    let mut rx = mount(two_card_game()).await;
    let section = last_section();
    click_in(&section, ".commit", 0);
    leptos::task::tick().await;
    let frame = rx
        .try_recv()
        .expect("a frame after tick — did tick flush the click handler?");
    assert_eq!(commit_indices(frame), Vec::<u32>::new());
}

#[wasm_bindgen_test]
async fn commit_after_selecting_submits_that_index() {
    let mut rx = mount(two_card_game()).await;
    let section = last_section();
    click_in(&section, ".hand-card", 0); // select index 0
    leptos::task::tick().await;
    click_in(&section, ".commit", 0);
    leptos::task::tick().await;
    let frame = rx
        .try_recv()
        .expect("a frame after tick — did tick flush the click handler?");
    assert_eq!(commit_indices(frame), vec![0]);
}

// ---- Setup mulligan via AwaitingInputView (#348 part 2c-iii-a) ----
//
// The setup mulligan is now an `AwaitingInput` routed through this same view
// (its dedicated picker was removed). The wrinkle vs. the commit window:
// during the mulligan loop there is no active investigator yet, so the view
// must fall back to the mulligan-prompted investigator's hand.

/// A setup-mulligan state: investigator 1 is on the mulligan loop with a
/// two-card hand and **no active investigator** (as the engine leaves it
/// during setup).
fn mulligan_game() -> game_core::state::GameState {
    let mut inv = test_investigator(1);
    inv.hand = vec![
        CardCode::new("_synth_event_a"),
        CardCode::new("_synth_event_b"),
    ];
    GameStateBuilder::new()
        .with_investigator(inv)
        .with_mulligan_remaining([InvestigatorId(1)])
        .build()
}

#[wasm_bindgen_test]
async fn mulligan_renders_prompted_hand_without_active_investigator() {
    // active_investigator is None during the mulligan loop; the view falls
    // back to current_mulligan() so the redraw hand still renders.
    let _rx = mount(mulligan_game()).await;
    let section = last_section();
    let html = section.inner_html();
    assert!(html.contains("_synth_event_a"), "card a missing: {html}");
    assert!(html.contains("_synth_event_b"), "card b missing: {html}");
}

#[wasm_bindgen_test]
async fn mulligan_submits_selected_redraw_index() {
    let mut rx = mount(mulligan_game()).await;
    let section = last_section();
    click_in(&section, ".hand-card", 0); // redraw index 0
    leptos::task::tick().await;
    click_in(&section, ".commit", 0);
    leptos::task::tick().await;
    let frame = rx
        .try_recv()
        .expect("a frame after tick — did tick flush the click handler?");
    assert_eq!(commit_indices(frame), vec![0]);
}

#[wasm_bindgen_test]
async fn hand_size_discard_renders_the_prompted_hand() {
    // Upkeep hand-size discard: NO active investigator, but a HandSizeDiscard
    // frame names inv 1; the prompted hand must still render (#468).
    let mut inv = test_investigator(1);
    inv.hand = vec![CardCode::new("01088"), CardCode::new("01089")];
    let base = GameStateBuilder::new().with_investigator(inv).build(); // NO active investigator
    let state =
        game_core::test_support::fixtures::at_hand_size_discard(base, vec![InvestigatorId(1)]);

    let _rx = mount(state).await;
    let section = last_section();
    let cards = section.query_selector_all(".hand-card").expect("query");
    assert_eq!(
        cards.length(),
        2,
        "the prompted investigator's 2 hand cards must render during hand-size discard"
    );
}
