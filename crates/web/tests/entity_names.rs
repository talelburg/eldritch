//! The board renders card/location *names*, not raw codes/ids.
//! wasm32-only (browser DOM). Runs against the real corpus
//! (`cards::TEST_REGISTRY`), the code→name source.
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support;
use leptos::prelude::{provide_context, RwSignal, Update};
use leptos::{mount, prelude, task};
use protocol::ServerMessage;
use wasm_bindgen_test::*;
use web::board::BoardView;
use web::registry::provide_cards;
use web::store::{self, ClientState};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn board_renders_card_and_location_names() {
    let inv_id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.hand.push(CardCode::new("01030")); // Magnifying Glass

    let state = GameStateBuilder::new()
        .with_active_investigator(inv_id)
        .with_investigator(inv)
        .with_location(test_support::test_location(10, "Study"))
        .build();

    let store = RwSignal::new(ClientState::default());
    mount::mount_to_body(move || {
        provide_cards(&cards::TEST_REGISTRY);
        provide_context(store);
        leptos::view! { <BoardView/> }
    });
    store.update(|s| {
        store::reduce(
            s,
            ServerMessage::Hello {
                state: Box::new(state),
                outcome: EngineOutcome::Done,
                events: Vec::new(),
            },
        );
    });
    task::tick().await;

    let text = prelude::document()
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
