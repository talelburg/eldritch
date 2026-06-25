//! Headless acceptance: feeding `Hello`/`Applied` updates the signal and
//! the DOM rendered from it. wasm32-only (browser DOM); native jobs skip.
#![cfg(target_arch = "wasm32")]

use game_core::state::GameStateBuilder;
use game_core::test_support::fixtures::test_investigator;
use game_core::EngineOutcome;
use leptos::prelude::Update;
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::board::BoardView;
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
    // Rendering the investigator panel reads `max_health()`/`max_sanity()`,
    // which resolve the investigator card's capacity from the registry (#448).
    // The fixture investigator uses the synthetic `TEST_INV` code (8/8).
    game_core::test_support::install_test_registry();
    let store = leptos::prelude::RwSignal::new(ClientState::default());
    // Provide the same signal the component reads, then mount the board;
    // it stays mounted (attached to the DOM) for the assertions.
    leptos::mount::mount_to_body(move || {
        leptos::prelude::provide_context(store);
        leptos::view! { <BoardView/> }
    });

    assert!(
        body_html().contains("&lt;no game&gt;"),
        "before: {}",
        body_html()
    );

    let game = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .build();
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(game),
                outcome: EngineOutcome::Done,
            },
        );
    });

    // CSR render effects flush on the next executor tick, not synchronously
    // with `update`. Yield so the DOM reflects the new signal value.
    leptos::task::tick().await;

    assert!(body_html().contains("phase-bar"), "after: {}", body_html());
}
