//! Headless render tests for the version-mismatch overlay (#770): the terminal
//! wire-format-skew state must announce itself over the board rather than as one
//! line in the header. wasm32-only.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::app::Overlays;
use web::store::{ClientState, ConnStatus};
use web::version_mismatch::VersionMismatchView;

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `VersionMismatchView` into a container of its own, and return that
/// container alongside the store. The DOM accumulates across tests in one
/// browser session, so a document-wide query would see the *other* test's
/// overlay; scoping every assertion to this test's container is what lets
/// "renders nothing" be asserted at all.
fn mount() -> (RwSignal<ClientState>, web_sys::HtmlElement) {
    let container = document()
        .create_element("div")
        .expect("create_element")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement");
    document()
        .body()
        .expect("body")
        .append_child(&container)
        .expect("append_child");

    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to(container.clone(), move || {
        provide_context(store);
        leptos::view! { <VersionMismatchView/> }
    })
    .forget();
    (store, container)
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
    ] {
        let (store, container) = mount();
        store.update(|s| s.status = status.clone());
        leptos::task::tick().await;

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
    leptos::task::tick().await;

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

/// The overlay is reached through the app's **real** overlay set, and is
/// declared after the others — the composition half of "above every other
/// overlay". The stacking itself is CSS (`.vm-backdrop`/`.version-mismatch` at
/// z-40/41 against the picker's 30 and the modal's 28/29) and `style.css` is not
/// loaded in this harness, so what is assertable here is that the overlay is in
/// the set at all and that it comes last in document order. That is the half
/// that regresses silently — a fourth overlay is easy to forget to compose, and
/// `Overlays` is the one place the app declares the order.
#[wasm_bindgen_test]
async fn the_real_overlay_set_declares_the_mismatch_card_last() {
    let store = RwSignal::new(ClientState::default());
    let container = document()
        .create_element("div")
        .expect("create_element")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("HtmlElement");
    document()
        .body()
        .expect("body")
        .append_child(&container)
        .expect("append_child");

    leptos::mount::mount_to(container.clone(), move || {
        provide_context(store);
        let pending = Signal::derive(move || store.with(web::interaction::pending_options));
        provide_context(web::interaction::PendingOptions(pending));
        let anchor = Signal::derive(move || store.with(web::interaction::confirm_anchor));
        provide_context(web::interaction::ConfirmAnchor(anchor));
        let selected = RwSignal::new(std::collections::BTreeSet::<u32>::new());
        let active = Signal::derive(move || store.with(web::interaction::is_multi_select));
        provide_context(web::interaction::MultiSelect { active, selected });
        leptos::view! { <Overlays/> }
    })
    .forget();

    // A live prompt as well as the mismatch, so the banner — the overlay
    // declared immediately before it — is actually in the DOM to be ordered
    // against.
    store.update(|s| {
        s.status = ConnStatus::VersionMismatch;
        s.outcome = Some(game_core::test_support::fixtures::awaiting_confirm_input(
            "Continue",
        ));
    });
    leptos::task::tick().await;

    let banner = container
        .query_selector(".prompt-banner")
        .expect("query")
        .expect("the prompt banner renders for a live prompt");
    let card = container
        .query_selector(".version-mismatch")
        .expect("query")
        .expect("the mismatch card renders from the real overlay set");

    // `DOCUMENT_POSITION_FOLLOWING` (4) — `card` comes after `banner`.
    assert_eq!(
        banner.compare_document_position(&card) & 4,
        4,
        "the mismatch card must be declared after the banner it has to cover"
    );
}
