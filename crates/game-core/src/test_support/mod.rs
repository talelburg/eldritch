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

/// Synthetic investigator-card code for unit tests. Registered by
/// [`install_test_registry`] with 8 health / 8 sanity (mirroring the legacy
/// `test_investigator` capacity).
pub const TEST_INV: &str = "TEST_INV";

fn test_inv_metadata() -> &'static crate::card_data::CardMetadata {
    use crate::card_data::{CardKind, CardMetadata, Class, Skills};
    static M: std::sync::OnceLock<CardMetadata> = std::sync::OnceLock::new();
    M.get_or_init(|| CardMetadata {
        code: TEST_INV.to_owned(),
        name: "Test Investigator".to_owned(),
        traits: vec![],
        text: None,
        pack_code: "_test".to_owned(),
        kind: CardKind::Investigator {
            class: Class::Neutral,
            skills: Skills {
                willpower: 3,
                intellect: 3,
                combat: 3,
                agility: 3,
            },
            health: 8,
            sanity: 8,
        },
    })
}

/// Install a minimal game-core test registry that knows `TEST_INV` (and only
/// it). Idempotent; safe to call from any test. Capacity-reading code
/// (`max_health()` / `max_sanity()` / soak / defeat) needs this installed.
pub fn install_test_registry() {
    use crate::state::CardCode;
    static INSTALL: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INSTALL.get_or_init(|| {
        fn metadata_for(code: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
            (code.as_str() == TEST_INV).then(test_inv_metadata)
        }
        fn abilities_for(_: &CardCode) -> Option<Vec<crate::dsl::Ability>> {
            None
        }
        let _ = crate::card_registry::install(crate::card_registry::CardRegistry {
            metadata_for,
            abilities_for,
            native_effect_for: |_| None,
        });
    });
}

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
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EnteredLocation {
            investigator,
            location,
        },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
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
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::PhaseEnded { phase },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire `ForcedTriggerPoint::RoundEnded` against `state`,
/// returning the `EngineOutcome`. See `fire_forced_on_enter`. Exercises
/// round-end Forced abilities (agenda 01107's doom).
pub fn fire_forced_on_round_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::RoundEnded,
        crate::dsl::EventTiming::At,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: run the Upkeep step-4.6 round-end sequence — `upkeep_phase_end`
/// then the `drive` loop that walks the `RoundEnded` coordinator (#434) —
/// returning the `EngineOutcome`. Suspends on act 01109's "when the round ends"
/// clue-spend reaction window when affordable; resume it with
/// [`resume_round_end_window`]. Requires the `UpkeepPhase` anchor on the stack
/// (the coordinator's teardown pops it).
pub fn run_upkeep_round_end(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::upkeep_phase_end(&mut cx);
    crate::engine::drive(&mut cx, out)
}

/// Test helper: resume the round-end `when` act-advance reaction window (#434)
/// with `response` (`PickSingle`/`Skip`), driving the coordinator through to its
/// next suspension or completion via the player-action entry.
pub fn resume_round_end_window(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    response: &crate::action::InputResponse,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    crate::engine::apply_player_action(
        &mut cx,
        &crate::action::PlayerAction::ResolveInput {
            response: response.clone(),
        },
    )
}

/// Test helper: fire forced triggers for an act advancing, returning the
/// `EngineOutcome`. See `fire_forced_on_enter`.
pub fn fire_forced_on_act_advance(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::ActAdvanced { code },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
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
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::AgendaAdvanced { code },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire forced triggers for an enemy defeat, returning the
/// `EngineOutcome`. See `fire_forced_on_enter`.
pub fn fire_forced_on_enemy_defeat(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    code: crate::state::CardCode,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EnemyDefeated { code },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
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
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::EndOfTurn { investigator },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}

/// Test helper: fire the forced phase of
/// `ForcedTriggerPoint::SkillTestResolved` (Investigate + success), returning
/// the `EngineOutcome`. See `fire_forced_on_enter`. Exercises the threat-area
/// "after successfully investigated" forced path via the controlled-instance
/// scan. It sets up no in-flight `SkillTest` frame, so the location-attachment
/// scan is a no-op here — the attachment path (Obscuring Fog 01168) is
/// exercised end-to-end through a real Investigate instead.
pub fn fire_forced_after_location_investigated(
    state: &mut crate::state::GameState,
    events: &mut Vec<crate::event::Event>,
    investigator: crate::state::InvestigatorId,
) -> crate::engine::EngineOutcome {
    let mut cx = crate::engine::Cx { state, events };
    let out = crate::engine::fire_forced_triggers(
        &mut cx,
        &crate::engine::ForcedTriggerPoint::SkillTestResolved {
            investigator,
            kind: crate::dsl::SkillTestKind::Investigate,
            outcome: crate::dsl::TestOutcome::Success,
        },
        crate::dsl::EventTiming::After,
    );
    crate::engine::drive(&mut cx, out)
}
