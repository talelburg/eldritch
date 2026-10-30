//! The scenarios crate's integration tests, built as one test binary.
//!
//! Each file in this directory is a module of the binary. A test that needs a
//! `MockRegistry` would be a top-level file under `tests/` instead, its own
//! binary: the mock's tables live in one process-wide `OnceLock`, so a second
//! mock in the same process panics.

mod issue_476_fast_window;
mod issue_482_advance;
mod test_registries;
mod the_gathering;
mod the_gathering_default_decks;
mod the_gathering_resolutions;
mod the_gathering_symbols;
