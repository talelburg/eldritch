//! #484: the board renders card/location *names*, not raw codes/ids.
//! wasm32-only (browser DOM). Own test binary so it can install the real
//! `cards::REGISTRY` (a code→name source) without colliding with other binaries.
#![cfg(target_arch = "wasm32")]

use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::fixtures::{test_investigator, test_location};
use game_core::EngineOutcome;
use leptos::prelude::{provide_context, RwSignal, Update};
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::store::{reduce, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn board_renders_card_and_location_names() {
    // Install the real corpus registry (the code→name source). Idempotent
    // (OnceLock, first-wins); `web` has no `ctor` dev-dep, so install in-test.
    let _ = game_core::card_registry::install(cards::REGISTRY);

    let inv_id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    // Use a real investigator code so the board's max-health/sanity reads resolve
    // against cards::REGISTRY (the synthetic TEST_INV code is absent from it).
    inv.investigator_card.code = CardCode::new("01001"); // Roland Banks
    inv.current_location = Some(LocationId(10));
    inv.hand.push(CardCode::new("01030")); // Magnifying Glass

    let state = GameStateBuilder::new()
        .with_active_investigator(inv_id)
        .with_investigator(inv)
        .with_location(test_location(10, "Study"))
        .build();

    let store = RwSignal::new(ClientState::default());
    leptos::mount::mount_to_body(move || {
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
            },
        );
    });
    leptos::task::tick().await;

    let text = leptos::prelude::document()
        .query_selector(".board")
        .expect("query")
        .expect(".board present")
        .text_content()
        .unwrap_or_default();
    assert!(
        text.contains("Magnifying Glass"),
        "hand card name shown: {text}"
    );
    assert!(
        !text.contains("01030"),
        "raw card code must not appear: {text}"
    );
    assert!(text.contains("Study"), "location name shown: {text}");
}
