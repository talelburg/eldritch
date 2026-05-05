//! Test-only support: fluent state builder, fixtures, event-assertion
//! macros.
//!
//! Available always inside `game-core`'s own tests (via `cfg(test)`)
//! and to downstream crates that enable the `test-support` feature.
//! The macros are exported at the crate root via `#[macro_export]`,
//! so callers see [`assert_event!`](crate::assert_event) regardless of
//! where they import the supporting types from.

pub mod assertions;
pub mod builder;
pub mod fixtures;

pub use builder::TestGame;
pub use fixtures::{test_investigator, test_location};
