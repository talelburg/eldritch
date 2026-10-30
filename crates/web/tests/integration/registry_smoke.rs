//! Smoke test: investigator capacity resolves from the real corpus, the way
//! board rendering reads it.
#![cfg(target_arch = "wasm32")]

use cards::TEST_REGISTRY;
use game_core::state::CardCode;
use game_core::test_support;
use wasm_bindgen_test::*;

/// Against `cards::TEST_REGISTRY` (the real corpus), verify that
/// `max_health()` / `max_sanity()` resolve correctly for Roland Banks (01001).
/// Stats verified against `data/arkhamdb-snapshot/pack/core/core.json`:
///   Roland Banks (01001): health = 9, sanity = 5.
#[wasm_bindgen_test]
fn roland_banks_capacity_resolves_with_real_registry() {
    let mut inv = test_support::test_investigator(1);
    inv.investigator_card.code = CardCode::new("01001");

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
