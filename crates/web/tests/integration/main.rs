//! The web crate's integration tests, as one test binary.
//!
//! Each file in this directory is a module of this root. Its own
//! `#![cfg(target_arch = …)]` line decides where it runs: the files gated
//! `not(target_arch = "wasm32")` build into the native binary that
//! `cargo test` runs, and the files gated `target_arch = "wasm32"` build into
//! the wasm binary that `wasm-pack test` runs in one headless-browser page.
//!
//! The browser tests share that page and nothing unmounts, so a test asserts
//! only on the root it mounted, never on the whole document.

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

mod act_agenda;
mod amulet_menu;
mod board;
mod board_is_the_input_surface;
mod board_surfaces;
mod card;
mod card_at_location;
#[cfg(target_arch = "wasm32")]
mod common;
mod context_menu;
mod encounter_deck;
mod enemy_card;
mod entity_names;
mod event_log;
mod in_play_card;
mod input_lockout;
mod location_card;
mod map;
mod map_node_layout;
mod panel_controls;
mod picker;
mod prompt_banner;
mod provisional_verdict;
mod registry_smoke;
mod sign_in;
mod skill_test_result;
mod skill_test_result_across_batches;
mod smoke;
mod status_bar;
mod store;
mod turn_tracker;
mod unresumable;
mod version_mismatch;
