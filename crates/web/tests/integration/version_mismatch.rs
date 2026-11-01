//! Regression for #770: the terminal wire-format-skew state announces itself
//! over the board, in the version-mismatch overlay, rather than as one line in
//! the header.
//!
//! wasm32-only.
#![cfg(target_arch = "wasm32")]

use game_core::test_support;
use leptos::prelude::*;
use leptos::task;
use wasm_bindgen_test::*;
use web::store::{ClientState, ConnStatus};
use web::version_mismatch::VersionMismatchView;
use web_sys::HtmlElement;

use crate::common;

/// Mount `VersionMismatchView` with [`common::mount`]. Scoping every
/// assertion to its container, rather than the shared document, is what lets
/// "renders nothing" be asserted at all.
fn mount() -> (RwSignal<ClientState>, HtmlElement) {
    common::mount(|| leptos::view! { <VersionMismatchView/> })
}

#[wasm_bindgen_test]
async fn overlay_is_absent_in_every_status_but_the_mismatch() {
    // Every variant but `VersionMismatch`, listed rather than sampled: the
    // acceptance criterion is "no overlay in *any* other status", and the guard
    // is one `!=`, so an added variant should be a compile-time decision here
    // rather than a silent gap.
    for status in [
        ConnStatus::Connecting,
        ConnStatus::Connected,
        ConnStatus::Reconnecting,
        ConnStatus::Failed,
        ConnStatus::AwaitingRoster,
        ConnStatus::SignedOut(vec![]),
    ] {
        let (store, container) = mount();
        store.update(|s| s.status = status.clone());
        task::tick().await;

        assert!(
            container
                .query_selector(".version-mismatch")
                .expect("query")
                .is_none(),
            "{status:?} must render no version-mismatch overlay"
        );
        assert!(
            container
                .query_selector(".vm-backdrop")
                .expect("query")
                .is_none(),
            "{status:?} must render no scrim over its board"
        );
    }
}

#[wasm_bindgen_test]
async fn version_mismatch_renders_a_scrimmed_card_naming_both_halves_of_the_fix() {
    let (store, container) = mount();
    store.update(|s| s.status = ConnStatus::VersionMismatch);
    task::tick().await;

    let card = container
        .query_selector(".version-mismatch")
        .expect("query")
        .expect("a .version-mismatch card");
    let html = card.inner_html();
    assert!(
        html.contains("version mismatch"),
        "the card must name the skew rather than look like a stalled engine: {html}"
    );
    assert!(
        html.contains("Restart the server"),
        "the card must name the half of the fix the button cannot do: {html}"
    );

    assert!(
        card.query_selector(".vm-reload").expect("query").is_some(),
        "the card must carry a reload control"
    );
    assert!(
        container
            .query_selector(".vm-backdrop")
            .expect("query")
            .is_some(),
        "the card must sit on a scrim, so the board behind it reads as inert"
    );
}

/// The overlay is reached through the app's **real** composition, and is
/// declared after the board's in-game modals — the composition half of "above
/// every other overlay". The stacking itself is CSS (`.vm-backdrop`/
/// `.version-mismatch` at z-40/41 against the picker's 30 and the modal's
/// 28/29) and `style.css` is not loaded in this harness, so what is assertable
/// here is that the overlay is in the composition at all and that it comes
/// after the lockout wrapper in document order, outside it. That is the half
/// that regresses silently — an overlay is easy to forget to compose, and
/// `MainColumn` is the one place the app declares the order.
#[wasm_bindgen_test]
async fn the_real_composition_declares_the_mismatch_card_after_the_board() {
    // A live prompt as well as the mismatch, so the prompt banner, an in-game
    // modal it has to cover, is actually in the DOM to be ordered against.
    let client = ClientState {
        status: ConnStatus::VersionMismatch,
        outcome: Some(test_support::awaiting_confirm_input("Continue")),
        ..ClientState::default()
    };
    let root = common::mount_board(client, &test_support::TEST_REGISTRY)
        .await
        .root;

    let lock = root
        .query_selector(".board-lock")
        .expect("query")
        .expect("the lockout wrapper");
    let banner = lock
        .query_selector(".prompt-banner")
        .expect("query")
        .expect("the prompt banner renders inside the wrapper for a live prompt");
    let card = root
        .query_selector(".version-mismatch")
        .expect("query")
        .expect("the mismatch card renders from the real composition");

    // `DOCUMENT_POSITION_FOLLOWING` (4) without `CONTAINED_BY` (16): `card`
    // comes after the wrapper, and not inside it.
    assert_eq!(
        lock.compare_document_position(&card) & (4 | 16),
        4,
        "the mismatch card must be declared after the wrapper, outside it"
    );
    assert_eq!(
        banner.compare_document_position(&card) & 4,
        4,
        "the mismatch card must be declared after the banner it has to cover"
    );
}
