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

/// Test helper: fire forced triggers for an investigator entering a
/// location, returning the `EngineOutcome`. Constructs the internal
/// `ForcedTriggerPoint` so integration tests don't need it public.
///
/// Lives in `test_support` because `fire_forced_triggers` needs a custom
/// `CardRegistry` and `OnceLock<CardRegistry>` is process-global — an
/// in-crate install would collide with `card_registry::tests`. Integration
/// tests in `crates/game-core/tests/` run in separate processes.
/// Wired into `move_action` (`EnteredLocation`); this helper exists for
/// unit-style coverage of the dispatch path in isolation.
pub fn fire_forced_on_enter(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
    location: crate::state::LocationId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        crate::engine::ForcedTriggerPoint::EnteredLocation {
            investigator,
            location,
        },
    )
}

/// Test helper: fire forced triggers for a phase ending, returning the
/// `EngineOutcome`. Constructs the internal `ForcedTriggerPoint` so
/// integration tests don't need it public. See `fire_forced_on_enter`.
pub fn fire_forced_on_phase_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    phase: crate::state::Phase,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        crate::engine::ForcedTriggerPoint::PhaseEnded { phase },
    )
}
