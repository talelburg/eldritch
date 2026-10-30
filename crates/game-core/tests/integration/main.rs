//! game-core's integration tests that run on a real or hand-built registry,
//! built as one test binary.
//!
//! Each file in this directory is a module of the binary. A test that builds a
//! `MockRegistry` stays a top-level file under `tests/` instead, its own
//! binary: the mock's tables live in one process-wide `OnceLock`, so a second
//! mock in the same process panics.

mod hunter_move_tie_break;
mod open_turn_builder;
mod player_order;
mod registries_per_apply;
mod resolution_module;
mod token_result;
mod upkeep_cascade;
mod upkeep_hand_size;
