//! Headless render tests for the version-mismatch overlay (#770): the terminal
//! wire-format-skew state must announce itself over the board rather than as one
//! line in the header. wasm32-only.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::{document, provide_context, RwSignal, Update};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
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
async fn overlay_is_absent_while_the_wire_format_agrees() {
    let (store, container) = mount();
    store.update(|s| s.status = ConnStatus::Connected);
    leptos::task::tick().await;

    assert!(
        container
            .query_selector(".version-mismatch")
            .expect("query")
            .is_none(),
        "a connected client must render no version-mismatch overlay"
    );
    assert!(
        container
            .query_selector(".vm-backdrop")
            .expect("query")
            .is_none(),
        "a connected client must render no scrim over its board"
    );
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
