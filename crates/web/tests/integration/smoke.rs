//! Headless smoke test proving the wasm-bindgen-test harness drives a real
//! browser against the Leptos `App`. Compiled only for wasm32 (crate-level
//! cfg below): the native `test`/`clippy`/`doc` jobs skip it entirely, so the
//! five pre-existing CI jobs are unaffected.
#![cfg(target_arch = "wasm32")]

use leptos::prelude::*;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::app::App;
use web::registry::provide_cards;
use web_sys::Element;

#[wasm_bindgen_test]
fn app_renders_greeting() {
    // Mount the app into the document body the way `main` does, registry
    // first, inside a `.smoke-root` container; it stays mounted (attached to
    // the DOM) for the assertion. The assertion reads that container, not the
    // whole body: the browser tests share one page and nothing unmounts.
    mount_to_body(|| {
        provide_cards(&cards::TEST_REGISTRY);
        view! { <div class="smoke-root"><App/></div> }
    });

    let roots = document().query_selector_all(".smoke-root").expect("query");
    let root = roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .smoke-root");

    assert!(
        root.inner_html().contains("Eldritch"),
        "rendered DOM should contain the greeting, got: {}",
        root.inner_html()
    );
}
