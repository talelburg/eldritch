//! The board renders card/location *names*, not raw codes/ids.
//!
//! wasm32-only (browser DOM). Runs against the real corpus
//! (`cards::TEST_REGISTRY`), the code→name source.
#![cfg(target_arch = "wasm32")]

use game_core::engine::EngineOutcome;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, location};
use wasm_bindgen_test::*;

use crate::common;

#[wasm_bindgen_test]
async fn board_renders_card_and_location_names() {
    let inv_id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(10));
    inv.hand.push(CardCode::new("01030")); // Magnifying Glass

    let state = GameStateBuilder::new()
        .with_active_investigator(inv_id)
        .with_investigator(inv)
        .with_location(location(10).name("Study"))
        .build();

    let root = common::mount_board(
        common::hello(state, EngineOutcome::Done),
        &cards::TEST_REGISTRY,
    )
    .await
    .root;

    let text = root
        .query_selector(".board")
        .expect("query")
        .expect("a .board")
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
