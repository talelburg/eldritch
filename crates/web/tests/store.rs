//! Headless acceptance: feeding `Hello`/`Applied` updates the signal and
//! the DOM rendered from it. wasm32-only (browser DOM); native jobs skip.
#![cfg(target_arch = "wasm32")]

use game_core::test_support::builder::TestGame;
use game_core::test_support::fixtures::test_investigator;
use game_core::EngineOutcome;
use leptos::prelude::Update;
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::app::DebugDump;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

fn body_html() -> String {
    leptos::prelude::document()
        .body()
        .expect("body")
        .inner_html()
}

#[wasm_bindgen_test]
async fn hello_renders_state_present() {
    let store = leptos::prelude::RwSignal::new(ClientState::default());
    // Provide the same signal the component reads, then mount the dump.
    let _handle = leptos::mount::mount_to_body(move || {
        leptos::prelude::provide_context(store);
        leptos::view! { <DebugDump/> }
    });

    assert!(
        body_html().contains("state: none"),
        "before: {}",
        body_html()
    );

    let game = TestGame::new()
        .with_investigator(test_investigator(1))
        .build();
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(game),
                outcome: EngineOutcome::Done,
            },
        )
    });

    // CSR render effects flush on the next executor tick, not synchronously
    // with `update`. Yield so the DOM reflects the new signal value.
    leptos::task::tick().await;

    assert!(
        body_html().contains("state: present"),
        "after: {}",
        body_html()
    );
}
