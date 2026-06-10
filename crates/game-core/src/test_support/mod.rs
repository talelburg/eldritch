//! Test-only support: fluent state builder, fixtures, event-assertion
//! macros.
//!
//! The macros are exported at the crate root via `#[macro_export]`,
//! so callers see [`assert_event!`](crate::assert_event) regardless
//! of where they import the supporting types from.

pub mod assertions;
pub mod builder;
pub mod fixtures;
pub mod resolver;

pub use builder::TestGame;
pub use fixtures::{awaiting_commit_input, test_enemy, test_investigator, test_location};
pub use resolver::{apply_no_commits, drive, ChoiceResolver, ScriptedResolver, TestSession};

// Re-export `ForcedTriggerPoint` so integration tests at
// `crates/game-core/tests/forced_triggers.rs` (a separate binary with
// their own `OnceLock<CardRegistry>`) can test `fire_forced_at` without
// reaching into the private `dispatch` module. The function is not yet
// wired into any action handler — Task 3 of #215 does the wiring.
pub use crate::engine::ForcedTriggerPoint;

/// Test helper: build a `Cx` from `state` and `events`, call
/// `fire_forced_triggers(point)`, and return the `EngineOutcome`.
///
/// Lives in `test_support` (rather than in-crate tests) because
/// `fire_forced_triggers` needs a custom `CardRegistry` and
/// `OnceLock<CardRegistry>` is process-global — an in-crate install
/// would collide with `card_registry::tests::install_is_idempotent`.
/// Integration tests in `crates/game-core/tests/` run in separate
/// processes, each with their own `OnceLock`, so collision is
/// impossible.
pub fn fire_forced_at(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    point: ForcedTriggerPoint,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(&mut cx, point)
}
