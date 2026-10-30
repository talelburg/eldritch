//! Headless acceptance: feeding `Hello`/`Applied` updates the signal and
//! the DOM rendered from it.
//!
//! wasm32-only (browser DOM); native jobs skip.
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::GameStateBuilder;
use game_core::test_support;
use leptos::prelude::{ClassAttribute, ElementChild, RwSignal, Update};
use leptos::{mount, prelude, task};
use protocol::ServerMessage;
use wasm_bindgen::JsCast as _;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::registry::provide_cards;
use web::store::{self, ClientState};
use web_sys::Element;

/// The HTML of the last `.store-root` container mounted: this test's own, not
/// the whole body, which the browser tests share and nothing unmounts.
fn root_html() -> String {
    let roots = prelude::document()
        .query_selector_all(".store-root")
        .expect("query");
    roots
        .item(roots.length() - 1)
        .and_then(|n| n.dyn_into::<Element>().ok())
        .expect("a .store-root")
        .inner_html()
}

#[wasm_bindgen_test]
async fn hello_renders_state_present() {
    // Rendering the investigator panel reads `max_health()`/`max_sanity()`,
    // which resolve the investigator card's capacity from the registry.
    // The fixture investigator uses the synthetic `TEST_INV` code (8/8).
    let store = RwSignal::new(ClientState::default());
    // Provide the same signal the component reads, then mount the board;
    // it stays mounted (attached to the DOM) for the assertions.
    mount::mount_to_body(move || {
        provide_cards(&test_support::TEST_REGISTRY);
        prelude::provide_context(store);
        leptos::view! { <div class="store-root"><BoardView/></div> }
    });

    assert!(
        root_html().contains("&lt;no game&gt;"),
        "before: {}",
        root_html()
    );

    let game = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    store.update(|s| {
        store::reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(game),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
    });

    // CSR render effects flush on the next executor tick, not synchronously
    // with `update`. Yield so the DOM reflects the new signal value.
    task::tick().await;

    // `phase_bar` was retired (phase/round moved to the turn tracker); assert the
    // game now renders its board — the investigators-panel heading.
    assert!(
        root_html().contains("Investigators"),
        "after: {}",
        root_html()
    );
}
