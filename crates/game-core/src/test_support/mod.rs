//! Test-only support: fixtures, event-assertion macros, and a
//! convenience re-export of the production [`GameStateBuilder`].
//!
//! The macros are exported at the crate root via `#[macro_export]`,
//! so callers see [`assert_event!`](crate::assert_event) regardless
//! of where they import the supporting types from.
//!
//! The state builder itself lives in [`crate::state`] (it constructs
//! production `GameState`s, not just test ones); it is re-exported here
//! so the existing test imports keep working.

pub mod assertions;
pub mod fixtures;
pub mod resolver;

pub use crate::state::GameStateBuilder;
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
        &crate::engine::ForcedTriggerPoint::EnteredLocation {
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
        &crate::engine::ForcedTriggerPoint::PhaseEnded { phase },
    )
}

/// Test helper: fire `ForcedTriggerPoint::RoundEnded` against `state`,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`. Exercises
/// round-end Forced abilities (agenda 01107's doom).
pub fn fire_forced_on_round_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(&mut cx, &crate::engine::ForcedTriggerPoint::RoundEnded)
}

/// Test helper: fire forced triggers for an act advancing, returning the
/// `EngineOutcome`. See `fire_forced_on_enter`.
pub fn fire_forced_on_act_advance(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::ActAdvanced { code },
    )
}

/// Test helper: fire forced triggers for an agenda advancing, returning
/// the `EngineOutcome`. See `fire_forced_on_enter`. Exercises the agenda
/// reverses (01105 discard/horror, 01106 dig-until-Ghoul).
pub fn fire_forced_on_agenda_advance(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::AgendaAdvanced { code },
    )
}

/// Test helper: fire forced triggers for an enemy defeat, returning the
/// `EngineOutcome`. See `fire_forced_on_enter`.
pub fn fire_forced_on_enemy_defeat(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EnemyDefeated { code },
    )
}

/// Test helper: fire `ForcedTriggerPoint::EndOfTurn` for `investigator`,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`. Exercises
/// the threat-area "at the end of your turn" forced path.
pub fn fire_forced_at_end_of_turn(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EndOfTurn { investigator },
    )
}

/// Test helper: fire `ForcedTriggerPoint::AfterLocationInvestigated`,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`. Exercises
/// the threat-area "after successfully investigated" forced path.
pub fn fire_forced_after_location_investigated(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
    location: crate::state::LocationId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::AfterLocationInvestigated {
            investigator,
            location,
        },
    )
}
