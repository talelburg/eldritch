//! Headless smoke test proving the wasm-bindgen-test harness drives a real
//! browser against the Leptos `App`. Compiled only for wasm32 (crate-level
//! cfg below): the native `test`/`clippy`/`doc` jobs skip it entirely, so the
//! five pre-existing CI jobs are unaffected.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use wasm_bindgen_test::*;
use web::app::App;
use web::registry::provide_cards;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn app_renders_greeting() {
    // Mount the app into the document body the way `main` does, registry
    // first; it stays mounted (attached to the DOM) for the assertion.
    mount_to_body(|| {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <App/> }
    });

    let body = document().body().expect("document should have a <body>");

    assert!(
        body.inner_html().contains("Eldritch"),
        "rendered DOM should contain the greeting, got: {}",
        body.inner_html()
    );
}
