//! Headless acceptance: feeding `Hello`/`Applied` updates the signal and
//! the DOM rendered from it.
//!
//! wasm32-only (browser DOM); native jobs skip.
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::GameStateBuilder;
use game_core::test_support;
use leptos::prelude::Update;
use leptos::task;
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::store::{self, ClientState};

use crate::common;

#[wasm_bindgen_test]
async fn hello_renders_state_present() {
    // Rendering the investigator panel reads `max_health()`/`max_sanity()`,
    // which resolve the investigator card's capacity from the registry.
    // The fixture investigator uses the synthetic `TEST_INV` code (8/8).
    // Mount on an empty store, then feed it a `Hello`; the board stays mounted
    // (attached to the DOM) for the assertions.
    let board = common::mount_board(ClientState::default(), &test_support::TEST_REGISTRY).await;
    let root_html = || board.root.inner_html();

    assert!(
        root_html().contains("&lt;no game&gt;"),
        "before: {}",
        root_html()
    );

    let game = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    board.store.update(|s| {
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
