//! Headless render tests for the app status bar (status / rejection / New game),
//! which now lives in the page header (moved out of `BoardView`). wasm32-only.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::{document, provide_context, RwSignal, Update};
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::status_bar::StatusBarView;
use web::store::{ClientState, ConnStatus};

wasm_bindgen_test_configure!(run_in_browser);

/// Mount `StatusBarView` against a fresh store; returns the store so a test can
/// drive `status`/`last_rejection`.
fn mount() -> RwSignal<ClientState> {
    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <StatusBarView/> }
    });
    store
}

#[wasm_bindgen_test]
async fn version_mismatch_status_renders_actionable_message() {
    let store = mount();
    store.update(|s| s.status = ConnStatus::VersionMismatch);
    leptos::task::tick().await;

    // Scope to the last mounted .status line (DOM accumulates across tests).
    let lines = document()
        .query_selector_all(".status")
        .expect("query_selector_all");
    let html = lines
        .item(lines.length() - 1)
        .expect("at least one .status line")
        .dyn_ref::<web_sys::Element>()
        .expect("Element")
        .inner_html();

    assert!(
        html.contains("version mismatch"),
        "status line must name the version mismatch: {html}"
    );
    assert!(
        html.contains("restart the server"),
        "status line must tell the user what to do: {html}"
    );
}

#[wasm_bindgen_test]
async fn status_bar_renders_a_new_game_button() {
    let _store = mount();
    leptos::task::tick().await;

    // Scope to the last mounted .status-bar (DOM accumulates across tests).
    let bars = document()
        .query_selector_all(".status-bar")
        .expect("query_selector_all");
    let last = bars
        .item(bars.length() - 1)
        .expect("at least one .status-bar")
        .dyn_into::<web_sys::Element>()
        .expect("Element");
    assert!(
        last.query_selector(".new-game").expect("query").is_some(),
        "status bar must render a .new-game button"
    );
}
