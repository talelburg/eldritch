//! Smoke test: investigator capacity resolves from the real corpus, the way
//! board rendering reads it.
#![cfg(target_arch = "wasm32")]

use cards::{TEST_REGISTRIES, TEST_REGISTRY};
use game_core::state::{GameStateBuilder, InvestigatorId};
use game_core::test_support::seat;
use wasm_bindgen_test::*;

/// Against `cards::TEST_REGISTRY` (the real corpus), verify that
/// `max_health()` / `max_sanity()` resolve correctly for Roland Banks (01001).
/// Stats verified against `data/arkhamdb-snapshot/pack/core/core.json`:
///   Roland Banks (01001): health = 9, sanity = 5.
#[wasm_bindgen_test]
fn roland_banks_capacity_resolves_with_real_registry() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card("01001"))
        .build();
    let inv = &state.investigators[&InvestigatorId(1)];

    assert_eq!(
        inv.max_health(&TEST_REGISTRY),
        9,
        "Roland Banks health should be 9"
    );
    assert_eq!(
        inv.max_sanity(&TEST_REGISTRY),
        5,
        "Roland Banks sanity should be 5"
    );
}
