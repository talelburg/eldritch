//! Headless smoke test proving the wasm-bindgen-test harness drives a real
//! browser against the Leptos `App`. Compiled only for wasm32 (crate-level
//! cfg below): the native `test`/`clippy`/`doc` jobs skip it entirely, so the
//! five pre-existing CI jobs are unaffected.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn app_renders_greeting() {
    // Mount the app into the document body; it stays mounted (attached to
    // the DOM) for the assertion.
    leptos::mount::mount_to_body(web::app::App);

    let body = leptos::prelude::document()
        .body()
        .expect("document should have a <body>");

    assert!(
        body.inner_html().contains("Eldritch"),
        "rendered DOM should contain the greeting, got: {}",
        body.inner_html()
    );
}
