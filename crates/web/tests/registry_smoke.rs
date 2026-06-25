//! Smoke test: `cards::REGISTRY` is installed at startup so investigator
//! capacity resolves without panic during board rendering (#448 C1).
//!
//! This binary installs the real `cards::REGISTRY` (not the synthetic one used
//! by `board.rs`). Each `tests/` file is a separate wasm-pack binary with its
//! own `OnceLock`, so the two registries do not collide.
#![cfg(target_arch = "wasm32")]

use game_core::state::CardCode;
use game_core::test_support::fixtures::test_investigator;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// Install `cards::REGISTRY` (the real corpus) and verify that
/// `max_health()` / `max_sanity()` resolve correctly for Roland Banks (01001).
/// Stats verified against `data/arkhamdb-snapshot/pack/core/core.json`:
///   Roland Banks (01001): health = 9, sanity = 5.
#[wasm_bindgen_test]
fn roland_banks_capacity_resolves_with_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);

    let mut inv = test_investigator(1);
    inv.investigator_card.code = CardCode::new("01001");

    assert_eq!(inv.max_health(), 9, "Roland Banks health should be 9");
    assert_eq!(inv.max_sanity(), 5, "Roland Banks sanity should be 5");
}
