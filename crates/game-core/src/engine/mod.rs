//! The engine: applies actions to game state, emits events.
//!
//! The central function here is [`apply`], which takes the current
//! state plus an [`Action`] and returns an [`ApplyResult`] containing
//! the new state, the events emitted, and an [`EngineOutcome`]
//! summarizing what happened.
//!
//! State changes happen exclusively through this function. The action
//! log persisted by the server is a flat sequence of [`Action`]s;
//! replaying it via [`apply`] from the initial state reproduces the
//! current state bit-for-bit.

mod cx;
pub use cx::Cx;
mod dispatch;
pub mod evaluator;
mod outcome;
pub(crate) mod pathfinding;

pub use dispatch::act_agenda::place_doom_on_current_agenda;
pub use dispatch::combat::deal_damage_to_enemy;
pub use dispatch::elimination::take_damage;
pub use dispatch::encounter::{
    reshuffle_encounter_discard, resolve_encounter_card, spawn_set_aside_enemy,
};
pub use dispatch::reveal::reveal_location;
pub use dispatch::threat_area::{attach_to_location, place_in_threat_area};
pub use evaluator::{effective_shroud, location_id_by_code, EvalContext};
pub use outcome::{EngineOutcome, InputRequest, ResumeToken};
pub use pathfinding::shortest_first_steps;

// Crate-internal re-exports for `test_support::fire_forced_on_enter`.
// Neither is public API: `ForcedTriggerPoint` stays internal; the
// integration test constructs it through the primitive-arg helper so
// it never needs to name the enum. `fire_forced_triggers` is wired into
// `move_action` (EnteredLocation) and `enemy_phase_end`/`upkeep_phase_end`
// (PhaseEnded).
pub(crate) use dispatch::forced_triggers::{fire_forced_triggers, ForcedTriggerPoint};

use crate::action::Action;
use crate::event::Event;
use crate::scenario::ScenarioRegistry;
use crate::state::GameState;

/// The result of a single [`apply`] call.
#[derive(Debug, Clone)]
#[must_use = "the post-apply GameState lives in ApplyResult.state; dropping the result drops the new state"]
#[non_exhaustive]
pub struct ApplyResult {
    /// The state after the action was applied. If the action was
    /// rejected, this is unchanged from the input state.
    pub state: GameState,
    /// Events emitted by the action's resolution. Empty if rejected.
    pub events: Vec<Event>,
    /// The terminal outcome of this apply call.
    pub outcome: EngineOutcome,
}

/// Apply a single action to the state.
///
/// Returns an [`ApplyResult`] containing the new state, events emitted,
/// and an [`EngineOutcome`] summarizing the result.
///
/// `apply` is the only entry point for state mutation; all changes flow
/// through here. It must be deterministic — same input state and
/// action always produce the same output — so the action log replays
/// cleanly.
///
/// # Handler contract
///
/// On [`EngineOutcome::Rejected`], the returned state and event list
/// are unchanged from the input. `apply` enforces this **structurally**:
/// it snapshots the state before dispatch and restores the snapshot on
/// rejection, and clears the (per-apply) event buffer. So no handler —
/// including the fallible-and-mutating DSL evaluator — can leak partial
/// state on rejection; handlers need not be defensively validate-first
/// for *correctness* of this invariant (they still should be, for clear
/// rejection messages and to avoid wasted work).
///
/// The transaction boundary is the `apply` *call*, not a multi-call
/// logical action: a reject during a
/// [`ResolveInput`](crate::action::PlayerAction::ResolveInput) rewinds to
/// the [`AwaitingInput`](EngineOutcome::AwaitingInput) pause state (the
/// input to that `apply`), not to before the original action — the pause
/// state was the product of an apply that returned `AwaitingInput`, whose
/// partial state is legitimate and retained.
///
/// On [`EngineOutcome::AwaitingInput`], the returned state and event
/// list reflect the work done up to the pause point — e.g. a
/// `PerformSkillTest` apply that suspends at the commit window has
/// already emitted [`Event::SkillTestStarted`] and populated
/// [`GameState::in_flight_skill_test`]. The resume action
/// ([`PlayerAction::ResolveInput`](crate::action::PlayerAction::ResolveInput))
/// drives the rest of resolution in a subsequent `apply` call. While
/// paused, every non-`ResolveInput` player action rejects.
pub fn apply(state: GameState, action: Action) -> ApplyResult {
    apply_with_scenario_registry(state, action, crate::scenario_registry::current())
}

/// Apply a single action with an explicit [`ScenarioRegistry`].
///
/// [`apply`] is the production entry point and reads the registry from
/// the global
/// [`scenario_registry::current`](crate::scenario_registry::current).
/// This variant exists so engine unit tests can drive the post-apply
/// resolution hook against a locally-constructed mock registry
/// without touching the process-global `OnceLock`.
///
/// The same firing rule applies regardless of how the registry is
/// supplied: a `Rejected` outcome clears events and skips the hook;
/// any non-`Rejected` outcome (`Done` or `AwaitingInput`) fires the
/// hook iff the resolution latch newly transitioned `None`->`Some`
/// this apply.
pub fn apply_with_scenario_registry(
    state: GameState,
    action: Action,
    registry: Option<&ScenarioRegistry>,
) -> ApplyResult {
    let mut state = state;
    let mut events = Vec::new();
    // Transactional snapshot: a Rejected outcome must leave the returned
    // state byte-identical to the input (the engine's "Rejected => state
    // unchanged" contract). Taken before any handler runs and restored
    // below if the outcome is Rejected, so no handler — including the
    // fallible-and-mutating DSL evaluator — can leak partial state on
    // rejection. AwaitingInput is untouched: it legitimately returns the
    // work done up to the pause point, so we restore on Rejected only.
    //
    // RNG state (`state.rng`) is part of the snapshot, so a rejected
    // action that advanced the RNG is rewound too. That's correct for
    // replay: a rejected action contributes nothing to the action log, so
    // it must contribute no RNG consumption either.
    let pristine = state.clone();
    let resolution_already_fired = state.resolution.is_some();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = match action {
            Action::Player(p) => dispatch::apply_player_action(&mut cx, &p),
            Action::Engine(e) => dispatch::apply_engine_record(&mut cx, &e),
        };
        if matches!(outcome, EngineOutcome::Rejected { .. }) {
            // Transactional restore (event half): the events buffer is
            // per-apply and starts empty, so clearing it == restoring it.
            // State half is restored after this block (the `cx` borrow on
            // `state` releases at the block close).
            cx.events.clear();
        } else if !resolution_already_fired {
            // A dispatch site may have latched a resolution this apply (act/
            // agenda resolution point, or no-remaining-players elimination).
            // Fire the module hook exactly once, on the None->Some transition.
            // Runs on Done AND AwaitingInput — a resolution can latch during
            // an apply that pauses (e.g. doom crosses the threshold in Mythos
            // 1.3 before the 1.4 draw pause).
            fire_scenario_resolution(&mut cx, registry);
        }
        outcome
        // `cx` drops here, releasing borrows on `state` and `events`.
    };
    // State half of the transactional restore: now that `cx`'s borrow on
    // `state` is released, swap the (possibly partially-mutated) state
    // back to the pristine snapshot on rejection.
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        state = pristine;
    }
    ApplyResult {
        state,
        events,
        outcome,
    }
}

/// Post-dispatch hook: if a dispatch site latched a resolution this apply
/// (`state.resolution` went `None`->`Some`), emit [`Event::ScenarioResolved`]
/// and run the active scenario module's `apply_resolution`. The caller
/// guards the `None`->`Some` transition (it reads `state.resolution.is_some()`
/// *before* dispatch), so this fires exactly once per scenario.
///
/// Short-circuits when no resolution latched. The `ScenarioResolved` event
/// is a property of engine state, so it fires even when no module is
/// registered (or `scenario_id` is `None`); only `apply_resolution` needs
/// the registry/module.
fn fire_scenario_resolution(cx: &mut Cx, registry: Option<&ScenarioRegistry>) {
    let Some(resolution) = cx.state.resolution.clone() else {
        return;
    };
    cx.events.push(Event::ScenarioResolved {
        resolution: resolution.clone(),
    });

    // Place victory-point locations in the victory display. Runs BEFORE
    // `(module.apply_resolution)(...)` so the scan captures board state
    // at the moment the resolution latches, before any post-resolution
    // cleanup (apply_resolution, Phase 9) runs. Generic across scenarios;
    // reads victory values from the card registry. No registry → no
    // metadata → nothing placed (graceful).
    //
    // RR p.21: "At the end of a scenario, place each victory point
    // location that is in play, revealed, and with no clues on it in the
    // victory display."
    if let Some(card_reg) = crate::card_registry::current() {
        let placed: Vec<(crate::state::CardCode, u8)> = cx
            .state
            .locations
            .values()
            .filter(|loc| loc.revealed && loc.clues == 0)
            .filter_map(|loc| {
                let meta = (card_reg.metadata_for)(&loc.code)?;
                match meta.kind {
                    crate::card_data::CardKind::Location {
                        victory: Some(v), ..
                    } if v > 0 => Some((loc.code.clone(), v)),
                    _ => None,
                }
            })
            .collect();
        for (code, victory) in placed {
            cx.state.victory_display.push(code.clone());
            cx.events
                .push(Event::EnteredVictoryDisplay { code, victory });
        }
    }

    // Fire game-end Forced abilities (Cover Up 01007's mental trauma, C5a
    // #236). Non-interactive in scope; a suspending GameEnd hit is #212
    // reentrancy work. Runs even when no scenario module is registered, so
    // it precedes the module lookup below.
    let _ = fire_forced_triggers(cx, &ForcedTriggerPoint::GameEnd);

    let Some(id) = cx.state.scenario_id.as_ref() else {
        return;
    };
    let Some(reg) = registry else { return };
    let Some(module) = (reg.module_for)(id) else {
        return;
    };
    (module.apply_resolution)(&resolution, cx.state, cx.events);
}

#[cfg(test)]
mod tests {
    use crate::action::{Action, EngineRecord, InputResponse, PlayerAction};
    use crate::event::{Event, FailureReason};
    use crate::state::EnemyId;
    use crate::state::{
        CardCode, ChaosToken, DefeatCause, GameState, InvestigatorId, Phase, SkillKind, Status,
        TokenModifiers, TokenResolution, Zone,
    };
    use crate::test_support::{
        apply_no_commits, test_enemy, test_investigator, test_location, GameStateBuilder,
    };
    use crate::{assert_event, assert_event_count, assert_no_event};

    use super::{apply, EngineOutcome};

    #[test]
    fn start_scenario_advances_to_investigation_with_round_one() {
        // StartScenario opens the mulligan window; the Investigation phase
        // does NOT begin until the last mulligan completes (Rules Reference
        // p.27: no action windows during setup; the game begins after
        // mulligans). After StartScenario alone, active_investigator is
        // None and no PhaseStarted(Investigation) fires yet.
        //
        // The full round-1 kickoff (active investigator set, PhaseStarted
        // fired) is covered by
        // `investigation_phase_tests::mulligan_completion_kicks_off_investigation_phase`.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();
        let start_result = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );

        assert_eq!(start_result.outcome, EngineOutcome::Done);
        assert_eq!(start_result.state.round, 1);
        assert_eq!(start_result.state.phase, Phase::Investigation);
        // Mulligan cursor is seeded — active investigator not yet set.
        assert_eq!(
            start_result.state.mulligan_pending,
            Some(id),
            "mulligan cursor must be seeded after StartScenario"
        );
        assert_eq!(
            start_result.state.active_investigator, None,
            "active investigator is not set until the mulligan cursor clears"
        );
        assert_eq!(start_result.state.investigators[&id].actions_remaining, 3);

        assert_event!(start_result.events, Event::ScenarioStarted);
        // Round 1: Mythos is skipped entirely — no PhaseStarted(Mythos) or
        // PhaseEnded(Mythos) fire (Rules Reference p.24: first round skips
        // the Mythos phase; the phase doesn't happen, not "runs empty").
        assert_no_event!(
            start_result.events,
            Event::PhaseStarted {
                phase: Phase::Mythos
            }
        );
        assert_no_event!(
            start_result.events,
            Event::PhaseEnded {
                phase: Phase::Mythos
            }
        );
        // PhaseStarted(Investigation) fires at mulligan completion, not here.
        assert_no_event!(
            start_result.events,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        );

        // After the sole investigator mulligans, the phase begins and the
        // lead becomes active.
        let mulligan_result = apply(
            start_result.state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(mulligan_result.outcome, EngineOutcome::Done);
        assert_eq!(
            mulligan_result.state.mulligan_pending, None,
            "cursor must clear"
        );
        assert_eq!(
            mulligan_result.state.active_investigator,
            Some(id),
            "lead investigator becomes active after mulligan window closes"
        );
        assert_event!(
            mulligan_result.events,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        );
        // rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2 / start_scenario seed);
        // actions_remaining == 3 is verified above via assert_eq.
    }

    #[test]
    fn start_scenario_on_already_started_state_is_rejected() {
        let state = GameStateBuilder::new().with_round(7).build();
        let result = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.round, 7);
        assert!(result.events.is_empty());
    }

    #[test]
    fn end_turn_drains_actions_and_emits_turn_ended() {
        let id = InvestigatorId(1);
        let mut roland = test_investigator(1);
        roland.actions_remaining = 3;
        let state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(roland)
            .with_active_investigator(id)
            .build();

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 0 } if *investigator == id
        );
        assert_event!(
            result.events,
            Event::TurnEnded { investigator } if *investigator == id
        );
        assert_eq!(result.state.investigators[&id].actions_remaining, 0);
    }

    #[test]
    fn end_turn_with_no_active_investigator_is_rejected() {
        let state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .build();

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn end_turn_outside_investigation_phase_is_rejected() {
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_phase(Phase::Mythos)
            .with_investigator(test_investigator(1))
            .with_active_investigator(id)
            .build();

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn rejected_actions_do_not_mutate_state() {
        let state = GameStateBuilder::new().build();
        let round_before = state.round;
        let phase_before = state.phase;
        let active_before = state.active_investigator;

        // ResolveInput is a Phase-1 stub — guaranteed to be Rejected.
        let result = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }),
        );

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_no_event!(result.events, _);
        assert_eq!(result.state.round, round_before);
        assert_eq!(result.state.phase, phase_before);
        assert_eq!(result.state.active_investigator, active_before);
    }

    #[test]
    fn guard_ladder_reject_leaves_state_byte_identical() {
        // A guard-ladder reject (ResolveInput against a state with no
        // in-flight skill test, no open windows, and no pending hunter
        // move) fires *before any mutation*. This locks that pre-mutation
        // rejects return the input state byte-identical (whole-state
        // equality, stronger than the field-by-field
        // `rejected_actions_do_not_mutate_state` above). The mid-resolution
        // rollback path — where a handler mutates *then* rejects — is
        // covered by `rejected_resolve_input_rewinds_to_pause_state_not_pre_action`
        // and the integration test in `crates/cards/tests/reject_rollback.rs`.
        let state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let before = state.clone();

        let result = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
        );

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(
            result.state, before,
            "rejected action must not mutate state"
        );
        assert!(result.events.is_empty());
    }

    #[test]
    fn rejected_resolve_input_rewinds_to_pause_state_not_pre_action() {
        // Drive a skill test to its commit-window AwaitingInput, then submit
        // a malformed response. The reject must rewind to the *pause* state
        // (in_flight_skill_test still set), not to before the skill test.
        let state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .with_chaos_bag(bag_only_zero())
            .build();

        let paused = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: InvestigatorId(1),
                skill: SkillKind::Willpower,
                difficulty: 2,
            }),
        );
        assert!(
            matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
            "skill test should pause at the commit window, got {:?}",
            paused.outcome,
        );
        assert!(paused.state.in_flight_skill_test.is_some());
        let s1 = paused.state.clone();

        // Malformed response: commit window expects CommitCards; send Skip.
        let result = apply(
            paused.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Skip,
            }),
        );

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(
            result.state, s1,
            "rejected ResolveInput rewinds to the pause state, not pre-action",
        );
        assert!(
            result.state.in_flight_skill_test.is_some(),
            "suspension stays open"
        );
        assert!(result.events.is_empty());
    }

    /// Standard-difficulty Night of the Zealot symbol-token values.
    fn night_of_the_zealot_standard() -> TokenModifiers {
        TokenModifiers {
            skull: -1,
            cultist: -2,
            tablet: -3,
            elder_thing: -4,
        }
    }

    /// Bag of a single `Numeric(0)` token — the next draw is always a
    /// no-op modifier, so test totals = skill exactly.
    fn bag_only_zero() -> crate::state::ChaosBag {
        crate::state::ChaosBag::new([ChaosToken::Numeric(0)])
    }

    #[test]
    fn perform_skill_test_with_unknown_investigator_is_rejected() {
        let state = GameStateBuilder::new()
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: InvestigatorId(999),
                skill: SkillKind::Willpower,
                difficulty: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn perform_skill_test_with_empty_bag_is_rejected() {
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Willpower,
                difficulty: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn perform_skill_test_with_negative_difficulty_is_rejected() {
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Willpower,
                difficulty: -1,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn perform_skill_test_succeeds_when_total_meets_difficulty() {
        // Default skills are 3/3/3/3; bag only has Numeric(0), so total=3.
        // Difficulty 3 → margin 0 → success.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::SkillTestStarted { investigator, skill: SkillKind::Intellect, difficulty: 3 }
                if *investigator == id
        );
        assert_event!(
            result.events,
            Event::ChaosTokenRevealed {
                token: ChaosToken::Numeric(0),
                resolution: TokenResolution::Modifier(0),
            }
        );
        assert_event!(
            result.events,
            Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
                if *investigator == id
        );
        assert_event!(
            result.events,
            Event::SkillTestEnded { investigator } if *investigator == id
        );
        assert_no_event!(result.events, Event::SkillTestFailed { .. });
    }

    #[test]
    fn perform_skill_test_succeeds_with_positive_margin() {
        // Skill 5 + Numeric(0) vs difficulty 2 → margin 3.
        let id = InvestigatorId(1);
        let mut strong = test_investigator(1);
        strong.skills.combat = 5;
        let state = GameStateBuilder::new()
            .with_investigator(strong)
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Combat,
                difficulty: 2,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::SkillTestSucceeded { investigator, skill: SkillKind::Combat, margin: 3 }
                if *investigator == id
        );
    }

    #[test]
    fn perform_skill_test_fails_when_total_below_difficulty() {
        // Skills 3/3/3/3, bag Numeric(0), difficulty 5 → margin -2 →
        // FailureReason::Total, by: 2.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Combat,
                difficulty: 5,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::SkillTestFailed {
                investigator,
                skill: SkillKind::Combat,
                reason: FailureReason::Total,
                by: 2,
            } if *investigator == id
        );
        assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
    }

    #[test]
    fn perform_skill_test_autofail_forces_total_to_zero() {
        // Per the Rules Reference: AutoFail makes the investigator's
        // total = 0 (not just "test fails"), so the failure margin is
        // computed against 0. Skill 99 + AutoFail vs difficulty 4 →
        // total 0, by = 4, reason AutoFail.
        let id = InvestigatorId(1);
        let mut high = test_investigator(1);
        high.skills.willpower = 99;
        let state = GameStateBuilder::new()
            .with_investigator(high)
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::AutoFail]))
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Willpower,
                difficulty: 4,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::SkillTestFailed {
                investigator,
                skill: SkillKind::Willpower,
                reason: FailureReason::AutoFail,
                by: 4,
            } if *investigator == id
        );
    }

    #[test]
    fn perform_skill_test_autofail_at_difficulty_zero_still_fails() {
        // Edge case: difficulty 0 would normally succeed at margin 0,
        // but AutoFail forces total = 0 AND tags the result as a
        // failure regardless. by = 0 here, reason = AutoFail.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::AutoFail]))
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Willpower,
                difficulty: 0,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::SkillTestFailed {
                reason: FailureReason::AutoFail,
                by: 0,
                ..
            }
        );
        assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
    }

    #[test]
    fn perform_skill_test_clamps_negative_total_to_zero() {
        // skill 3 + Skull(−6) = −3, clamped to 0. Difficulty 2 →
        // by = 2, reason Total (NOT AutoFail).
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::Skull]))
            .with_token_modifiers(TokenModifiers {
                skull: -6,
                ..TokenModifiers::default()
            })
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Willpower,
                difficulty: 2,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::SkillTestFailed {
                reason: FailureReason::Total,
                by: 2,
                ..
            }
        );
    }

    #[test]
    fn perform_skill_test_elder_sign_treated_as_modifier_zero() {
        // ElderSign as +0 placeholder until per-investigator ability
        // dispatch lands. Skill 3, difficulty 3 → margin 0 → success.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::ElderSign]))
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Agility,
                difficulty: 3,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ChaosTokenRevealed {
                token: ChaosToken::ElderSign,
                resolution: TokenResolution::ElderSign,
            }
        );
        assert_event!(result.events, Event::SkillTestSucceeded { margin: 0, .. });
    }

    #[test]
    fn perform_skill_test_symbol_token_modifier_applies() {
        // Bag is one Skull. Standard-difficulty NotZ: skull = -1.
        // Skill 3 + (-1) = 2 vs difficulty 2 → margin 0 → success.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::Skull]))
            .with_token_modifiers(night_of_the_zealot_standard())
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Willpower,
                difficulty: 2,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ChaosTokenRevealed {
                token: ChaosToken::Skull,
                resolution: TokenResolution::Modifier(-1),
            }
        );
        assert_event!(result.events, Event::SkillTestSucceeded { margin: 0, .. });
    }

    #[test]
    fn perform_skill_test_drains_only_resolving_investigators_pending_modifiers() {
        // Two investigators each have a pending ThisSkillTest entry.
        // Running a skill test for inv1 must drain inv1's entry but
        // leave inv2's intact for their own future test.
        let id1 = InvestigatorId(1);
        let id2 = InvestigatorId(2);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_chaos_bag(bag_only_zero())
            .build();
        state.pending_skill_modifiers = vec![
            crate::state::PendingSkillModifier {
                investigator: id1,
                stat: crate::dsl::Stat::Willpower,
                delta: 1,
                source: None,
            },
            crate::state::PendingSkillModifier {
                investigator: id2,
                stat: crate::dsl::Stat::Willpower,
                delta: 1,
                source: None,
            },
        ];

        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id1,
                skill: SkillKind::Willpower,
                difficulty: 0,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.state.pending_skill_modifiers.len(),
            1,
            "inv2's entry must survive inv1's test",
        );
        assert_eq!(result.state.pending_skill_modifiers[0].investigator, id2);
    }

    #[test]
    fn perform_skill_test_advances_rng_and_log_round_trips() {
        // Determinism: applying the same PerformSkillTest action twice
        // from identical initial state produces identical post-state.
        let id = InvestigatorId(1);
        let initial = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([
                ChaosToken::Numeric(1),
                ChaosToken::Numeric(-1),
                ChaosToken::Skull,
            ]))
            .with_token_modifiers(night_of_the_zealot_standard())
            .with_rng_seed(123)
            .build();
        let action = Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Willpower,
            difficulty: 3,
        });

        let first = apply_no_commits(initial.clone(), action.clone());
        let second = apply_no_commits(initial, action);

        assert_eq!(first.outcome, EngineOutcome::Done);
        assert_eq!(first.state.rng, second.state.rng);
        assert_eq!(first.state.rng.draws, 1);
        assert_eq!(first.events, second.events);
    }

    /// Build a scenario suitable for Investigate tests: one investigator
    /// at a location with `clues` clues and `shroud` shroud, in
    /// Investigation phase, with the investigator active and 3 actions.
    /// Bag is `Numeric(0)` so the test outcome depends purely on
    /// (intellect vs shroud).
    fn investigate_scenario(
        clues: u8,
        shroud: u8,
    ) -> (InvestigatorId, crate::state::LocationId, GameState) {
        let inv_id = InvestigatorId(1);
        let loc_id = crate::state::LocationId(10);
        let mut inv = test_investigator(1);
        inv.current_location = Some(loc_id);
        inv.actions_remaining = 3;
        let mut loc = test_location(10, "Study");
        loc.clues = clues;
        loc.shroud = shroud;
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .with_chaos_bag(bag_only_zero())
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();
        (inv_id, loc_id, state)
    }

    #[test]
    fn investigate_succeeds_and_moves_one_clue_to_investigator() {
        // Default intellect 3, shroud 2 → margin 1 → success.
        let (inv_id, loc_id, state) = investigate_scenario(2, 2);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::SkillTestStarted {
                skill: SkillKind::Intellect,
                difficulty: 2,
                ..
            }
        );
        assert_event!(result.events, Event::SkillTestSucceeded { margin: 1, .. });
        assert_event!(
            result.events,
            Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::LocationCluesChanged { location, new_count: 1 } if *location == loc_id
        );
        assert_eq!(result.state.investigators[&inv_id].clues, 1);
        assert_eq!(result.state.locations[&loc_id].clues, 1);
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    }

    #[test]
    fn investigate_failure_spends_action_but_moves_no_clue() {
        // Intellect 3, shroud 5 → fails by 2; action still spent.
        let (inv_id, loc_id, state) = investigate_scenario(2, 5);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::SkillTestFailed { by: 2, .. });
        assert_no_event!(result.events, Event::CluePlaced { .. });
        assert_no_event!(result.events, Event::LocationCluesChanged { .. });
        assert_eq!(result.state.locations[&loc_id].clues, 2);
        assert_eq!(result.state.investigators[&inv_id].clues, 0);
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    }

    #[test]
    fn investigate_at_empty_location_spends_action_and_runs_test_silently() {
        // Location has 0 clues; the test still fires (you can't tell
        // the location is empty without trying), the action is still
        // spent, and discover_clue is a silent no-op on success.
        let (inv_id, loc_id, state) = investigate_scenario(0, 2);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::SkillTestSucceeded { .. });
        assert_no_event!(result.events, Event::CluePlaced { .. });
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
        assert_eq!(result.state.locations[&loc_id].clues, 0);
        assert_eq!(result.state.investigators[&inv_id].clues, 0);
    }

    #[test]
    fn investigate_outside_investigation_phase_is_rejected() {
        let (inv_id, _, mut state) = investigate_scenario(2, 2);
        state.phase = Phase::Mythos;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn investigate_by_non_active_investigator_is_rejected() {
        let (_, _, mut state) = investigate_scenario(2, 2);
        // Add a second investigator but keep the first active.
        let other = InvestigatorId(2);
        state.investigators.insert(other, test_investigator(2));
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: other,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn investigate_with_zero_actions_is_rejected() {
        let (inv_id, _, mut state) = investigate_scenario(2, 2);
        state
            .investigators
            .get_mut(&inv_id)
            .unwrap()
            .actions_remaining = 0;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn investigate_without_a_current_location_is_rejected() {
        let (inv_id, _, mut state) = investigate_scenario(2, 2);
        state
            .investigators
            .get_mut(&inv_id)
            .unwrap()
            .current_location = None;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    // After T09, end_turn pauses at Mythos when mythos_draw_pending is
    // Some(_). The player-driven DrawEncounterCard action (T12) is what
    // completes the Mythos phase; it requires a card registry which
    // game-core unit tests cannot install (process-global OnceLock;
    // installing in one test would contaminate others). The full
    // round-cycle coverage — including DrawEncounterCard completing the
    // phase and transitioning to Investigation — lives in the T14
    // integration tests at crates/scenarios/tests/mythos_phase.rs.
    //
    // These two tests verify the pause-at-Mythos shape: the last EndTurn
    // in a round must land in Mythos with mythos_draw_pending populated,
    // with the correct partial event chain (Investigation/Enemy/Upkeep
    // boundaries + PhaseStarted(Mythos)). PhaseEnded(Mythos) and
    // PhaseStarted(Investigation) do NOT fire here — they fire later via
    // the DrawEncounterCard → mythos_phase_end continuation.
    #[test]
    fn last_end_turn_advances_to_mythos_and_pauses_for_draw_two_investigators() {
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_turn_order([inv1, inv2])
            .build();

        // StartScenario: round 0 → 1, phase Investigation (mulligan window
        // open). The Investigation phase does NOT begin until the last
        // investigator mulligans — active_investigator is None here.
        let result = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );
        let state = result.state;
        assert_eq!(state.round, 1);
        assert_eq!(state.phase, Phase::Investigation);
        assert_eq!(
            state.mulligan_pending,
            Some(inv1),
            "mulligan cursor must be seeded after StartScenario"
        );
        assert_eq!(
            state.active_investigator, None,
            "active investigator not yet set — Investigation phase begins after mulligan"
        );
        assert_eq!(state.investigators[&inv1].actions_remaining, 3);

        // Mulligan past the window for both investigators (empty
        // redraws = "keep my hand"). The engine requires every
        // investigator to mulligan before non-Mulligan actions are
        // accepted.
        let state = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
        )
        .state;
        let state = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        )
        .state;
        // After the last mulligan, the Investigation phase begins and the
        // lead investigator becomes active.
        assert_eq!(
            state.active_investigator,
            Some(inv1),
            "lead investigator becomes active after mulligan window closes"
        );

        // First EndTurn (inv1): rotates to inv2 within Investigation.
        // No phase transitions yet.
        let result = apply(state, Action::Player(PlayerAction::EndTurn));
        let state = result.state;
        assert_eq!(state.round, 1);
        assert_eq!(state.phase, Phase::Investigation);
        assert_eq!(state.active_investigator, Some(inv2));
        assert_eq!(state.investigators[&inv1].actions_remaining, 0);
        assert_eq!(state.investigators[&inv2].actions_remaining, 3);
        assert_event!(
            result.events,
            Event::TurnEnded { investigator } if *investigator == inv1
        );
        // rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2)
        assert_no_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, .. } if *investigator == inv2
        );
        // No phase transition on a mid-round EndTurn.
        assert_no_event!(result.events, Event::PhaseEnded { .. });
        assert_no_event!(result.events, Event::PhaseStarted { .. });
        assert_no_event!(result.events, Event::ScenarioStarted);

        // Second EndTurn (inv2, last in turn_order): auto-advances through
        // Investigation → Enemy → Upkeep → Mythos and then PAUSES because
        // mythos_draw_pending is now Some(inv1). The phase chain does NOT
        // continue to Investigation — that waits for DrawEncounterCard.
        let result = apply(state, Action::Player(PlayerAction::EndTurn));
        let state = result.state;
        assert_eq!(state.round, 2, "round bumps on Mythos entry");
        assert_eq!(state.phase, Phase::Mythos);
        assert_eq!(
            state.mythos_draw_pending,
            Some(inv1),
            "lead investigator (inv1) draws first"
        );
        assert_eq!(result.outcome, EngineOutcome::Done);

        // Exactly 3 PhaseEnded events fire (Investigation, Enemy, Upkeep).
        // PhaseEnded(Mythos) does NOT fire here — mythos_phase_end owns it
        // and runs only after DrawEncounterCard completes the chain.
        assert_event_count!(result.events, 3, Event::PhaseEnded { .. });
        for phase in [Phase::Investigation, Phase::Enemy, Phase::Upkeep] {
            assert_event!(result.events, Event::PhaseEnded { phase: p } if *p == phase);
        }
        assert_no_event!(
            result.events,
            Event::PhaseEnded { phase: p } if *p == Phase::Mythos
        );

        // Exactly 3 PhaseStarted events fire (Enemy, Upkeep, Mythos).
        // PhaseStarted(Investigation) does NOT fire here — investigation_phase
        // runs only after mythos_phase_end, which runs after DrawEncounterCard.
        assert_event_count!(result.events, 3, Event::PhaseStarted { .. });
        for phase in [Phase::Enemy, Phase::Upkeep, Phase::Mythos] {
            assert_event!(result.events, Event::PhaseStarted { phase: p } if *p == phase);
        }
        assert_no_event!(
            result.events,
            Event::PhaseStarted { phase: p } if *p == Phase::Investigation
        );

        // EndTurn must never re-emit ScenarioStarted.
        assert_no_event!(result.events, Event::ScenarioStarted);
    }

    #[test]
    fn last_end_turn_advances_to_mythos_and_pauses_for_draw_solo() {
        // Degenerate edge: with only one investigator in turn_order,
        // their single EndTurn is also the *last* EndTurn of the round.
        // It must auto-advance Investigation → Enemy → Upkeep → Mythos,
        // bump the round, seed mythos_draw_pending = Some(id), and then
        // PAUSE. It does NOT complete the full cycle — that requires the
        // subsequent DrawEncounterCard action (needs registry, covered by
        // crates/scenarios/tests/mythos_phase.rs).
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();

        // StartScenario: round 0 → 1, mulligan window opens.
        // active_investigator is None until mulligan completion.
        let after_start = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        )
        .state;
        assert_eq!(after_start.round, 1);
        assert_eq!(
            after_start.active_investigator, None,
            "active investigator not set until mulligan window closes"
        );

        // Mulligan past the setup window. After completion, lead becomes active.
        let after_mulligan = apply(
            after_start,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![],
            }),
        )
        .state;

        let result = apply(after_mulligan, Action::Player(PlayerAction::EndTurn));
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.round, 2, "round bumps on Mythos entry");
        assert_eq!(result.state.phase, Phase::Mythos);
        assert_eq!(
            result.state.mythos_draw_pending,
            Some(id),
            "sole investigator is the pending drawer"
        );

        // The partial event chain: 3 PhaseEnded (Investigation, Enemy, Upkeep)
        // and 3 PhaseStarted (Enemy, Upkeep, Mythos). The Mythos-side
        // pair (PhaseEnded(Mythos) + PhaseStarted(Investigation)) fires
        // later via mythos_phase_end after DrawEncounterCard resolves.
        assert_event_count!(result.events, 3, Event::PhaseEnded { .. });
        assert_event_count!(result.events, 3, Event::PhaseStarted { .. });
        for phase in [Phase::Investigation, Phase::Enemy, Phase::Upkeep] {
            assert_event!(result.events, Event::PhaseEnded { phase: p } if *p == phase);
        }
        assert_no_event!(
            result.events,
            Event::PhaseEnded { phase: p } if *p == Phase::Mythos
        );
        for phase in [Phase::Enemy, Phase::Upkeep, Phase::Mythos] {
            assert_event!(result.events, Event::PhaseStarted { phase: p } if *p == phase);
        }
        assert_no_event!(
            result.events,
            Event::PhaseStarted { phase: p } if *p == Phase::Investigation
        );
    }

    #[test]
    fn deck_shuffled_engine_record_with_unknown_investigator_is_rejected() {
        let state = GameStateBuilder::new().build();
        let result = apply(
            state,
            Action::Engine(EngineRecord::DeckShuffled {
                investigator: InvestigatorId(999),
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    // ------------------------------------------------------------------
    // Player deck / zone tests (#62)
    // ------------------------------------------------------------------

    /// Build a deck of `n` cards with codes "test-001", "test-002",
    /// etc. so tests can identify exact ordering.
    fn make_test_deck(n: usize) -> Vec<CardCode> {
        (1..=n).map(|i| CardCode(format!("test-{i:03}"))).collect()
    }

    #[test]
    fn start_scenario_shuffles_each_deck_and_deals_initial_hand() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = make_test_deck(10);
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .with_rng_seed(42)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::DeckShuffled { investigator } if *investigator == id
        );
        assert_event!(
            result.events,
            Event::CardsDrawn { investigator, count: 5 } if *investigator == id
        );
        // Hand has 5 cards, deck has 5 left, both partitions cover the
        // original 10 cards (just shuffled).
        let inv_after = &result.state.investigators[&id];
        assert_eq!(inv_after.hand.len(), 5);
        assert_eq!(inv_after.deck.len(), 5);
        let mut all: Vec<_> = inv_after.hand.iter().chain(inv_after.deck.iter()).collect();
        all.sort();
        let mut expected: Vec<_> = make_test_deck(10).into_iter().collect();
        expected.sort();
        assert_eq!(
            all.iter().map(|c| CardCode::as_str(c)).collect::<Vec<_>>(),
            expected.iter().map(CardCode::as_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn start_scenario_with_empty_deck_yields_empty_hand_and_no_events() {
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        // Empty-deck no-op shuffle: no event.
        assert_no_event!(result.events, Event::DeckShuffled { .. });
        // draw_cards still emits CardsDrawn { count: 0 } so consumers
        // see the attempt.
        assert_event!(
            result.events,
            Event::CardsDrawn { investigator, count: 0 } if *investigator == id
        );
        assert!(result.state.investigators[&id].hand.is_empty());
        assert!(result.state.investigators[&id].deck.is_empty());
    }

    #[test]
    fn start_scenario_with_short_deck_draws_only_what_remains() {
        // Deck of 3, INITIAL_HAND_SIZE is 5: draw 3, deck empties, no
        // panic.
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = make_test_deck(3);
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .with_rng_seed(7)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::CardsDrawn { investigator, count: 3 } if *investigator == id
        );
        assert_eq!(result.state.investigators[&id].hand.len(), 3);
        assert!(result.state.investigators[&id].deck.is_empty());
    }

    #[test]
    fn deck_shuffle_is_deterministic_across_replay() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = make_test_deck(20);
        let state_a = GameStateBuilder::new()
            .with_investigator(inv.clone())
            .with_turn_order([id])
            .with_rng_seed(123)
            .build();
        let state_b = GameStateBuilder::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .with_rng_seed(123)
            .build();

        let result_a = apply(
            state_a,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );
        let result_b = apply(
            state_b,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );

        assert_eq!(
            result_a.state.investigators[&id].deck,
            result_b.state.investigators[&id].deck
        );
        assert_eq!(
            result_a.state.investigators[&id].hand,
            result_b.state.investigators[&id].hand
        );
        assert_eq!(result_a.state.rng, result_b.state.rng);
    }

    #[test]
    fn deck_shuffled_engine_record_shuffles_named_investigator() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = make_test_deck(8);
        let original_deck = inv.deck.clone();
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_rng_seed(99)
            .build();
        let result = apply(
            state,
            Action::Engine(EngineRecord::DeckShuffled { investigator: id }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::DeckShuffled { investigator } if *investigator == id
        );
        // Deck contains the same cards (multiset equal) but reordered.
        // With seed 99 and 8 cards, the shuffle should differ from
        // the original; treat that as a probabilistic check.
        let after = &result.state.investigators[&id].deck;
        assert_eq!(after.len(), original_deck.len());
        let mut sorted_before = original_deck.clone();
        sorted_before.sort();
        let mut sorted_after = after.clone();
        sorted_after.sort();
        assert_eq!(sorted_before, sorted_after);
    }

    #[test]
    fn start_scenario_handles_sparse_investigator_ids_deterministically() {
        // Investigator ids 1, 5, 9 — non-contiguous. BTreeMap
        // iteration is sorted, so shuffle order is deterministic.
        // Each investigator gets their own deck + hand independently.
        let ids = [InvestigatorId(1), InvestigatorId(5), InvestigatorId(9)];
        let mut tg = GameStateBuilder::new().with_rng_seed(2026);
        for id in ids {
            let mut inv = test_investigator(id.0);
            inv.deck = make_test_deck(8);
            tg = tg.with_investigator(inv);
        }
        let result = apply(
            tg.build(),
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);

        // Each investigator drew 5 cards and has 3 left in deck.
        for id in ids {
            let inv_after = &result.state.investigators[&id];
            assert_eq!(inv_after.hand.len(), 5);
            assert_eq!(inv_after.deck.len(), 3);
        }
        // Each emitted CardsDrawn { count: 5 }.
        assert_event_count!(result.events, 3, Event::CardsDrawn { count: 5, .. });
    }

    #[test]
    #[should_panic(expected = "state-corruption invariant violation")]
    fn investigate_with_dangling_current_location_panics() {
        // Corruption case: investigator's current_location references
        // a location not in state.locations. Matches the loud-on-
        // corruption pattern used by end_turn / rotate_to_active.
        let (inv_id, loc_id, mut state) = investigate_scenario(2, 2);
        state.locations.remove(&loc_id);
        let _ = apply(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
    }

    /// Build a scenario suitable for Move tests: one investigator at
    /// location A, with A connected to B (and only A→B; B has no
    /// connections back). Investigation phase, active investigator,
    /// 3 actions. Returns (investigator id, A id, B id, state).
    fn move_scenario() -> (
        InvestigatorId,
        crate::state::LocationId,
        crate::state::LocationId,
        GameState,
    ) {
        let inv_id = InvestigatorId(1);
        let a = crate::state::LocationId(10);
        let b = crate::state::LocationId(11);
        let mut inv = test_investigator(1);
        inv.current_location = Some(a);
        inv.actions_remaining = 3;
        let mut loc_a = test_location(10, "A");
        loc_a.connections = vec![b];
        let loc_b = test_location(11, "B");
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();
        (inv_id, a, b, state)
    }

    #[test]
    fn move_to_connected_location_spends_action_and_emits_events() {
        let (inv_id, a, b, state) = move_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::InvestigatorMoved { investigator, from, to }
                if *investigator == inv_id && *from == a && *to == b
        );
        assert_eq!(
            result.state.investigators[&inv_id].current_location,
            Some(b)
        );
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    }

    #[test]
    fn move_to_unconnected_location_is_rejected() {
        // Build a fresh scenario where C exists but A is not connected to C.
        let (inv_id, _, _, mut state) = move_scenario();
        let c = crate::state::LocationId(12);
        state.locations.insert(c, test_location(12, "C"));
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: c,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn move_to_current_location_is_rejected() {
        let (inv_id, a, _, state) = move_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: a,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn move_outside_investigation_phase_is_rejected() {
        let (inv_id, _, b, mut state) = move_scenario();
        state.phase = Phase::Mythos;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn move_by_non_active_investigator_is_rejected() {
        let (_, _, b, mut state) = move_scenario();
        let other = InvestigatorId(2);
        state.investigators.insert(other, test_investigator(2));
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: other,
                destination: b,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn move_with_zero_actions_is_rejected() {
        let (inv_id, _, b, mut state) = move_scenario();
        state
            .investigators
            .get_mut(&inv_id)
            .unwrap()
            .actions_remaining = 0;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn move_without_current_location_is_rejected() {
        let (inv_id, _, b, mut state) = move_scenario();
        state
            .investigators
            .get_mut(&inv_id)
            .unwrap()
            .current_location = None;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn move_to_missing_destination_is_rejected() {
        // Connection list points to an id that's been removed from
        // state.locations — this isn't state corruption (the
        // current_location is intact), it's a malformed connection
        // graph the caller might fix; reject.
        let (inv_id, _a, b, mut state) = move_scenario();
        state.locations.remove(&b);
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    #[should_panic(expected = "state-corruption invariant violation")]
    fn move_with_dangling_current_location_panics() {
        // Corruption: current_location points at A but A isn't in
        // state.locations. Surface loudly per the project pattern.
        let (inv_id, a, b, mut state) = move_scenario();
        state.locations.remove(&a);
        let _ = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
    }

    #[test]
    #[should_panic(expected = "state-corruption invariant violation")]
    fn move_with_active_investigator_missing_from_map_panics() {
        // Corruption: active_investigator points at an id that isn't
        // in state.investigators. The active-investigator check passes
        // (Some(id) == active), so this case is only reachable from
        // corrupt state — panic to match end_turn / rotate_to_active.
        let (inv_id, _a, b, mut state) = move_scenario();
        state.investigators.remove(&inv_id);
        let _ = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
    }

    #[test]
    fn move_to_a_set_aside_location_is_rejected() {
        use crate::state::{CardCode, Location, LocationId};
        let (inv_id, a, _b, mut state) = move_scenario();
        // A location that exists only in the set-aside zone is out of play.
        state.set_aside_locations.push(Location::new(
            LocationId(99),
            CardCode("setaside".into()),
            "Aside",
            1,
            0,
        ));
        // Illegally connect the current location to it; the move must STILL be rejected (not in play).
        state
            .locations
            .get_mut(&a)
            .unwrap()
            .connections
            .push(LocationId(99));
        let r = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: LocationId(99),
            }),
        );
        assert!(
            matches!(r.outcome, EngineOutcome::Rejected { .. }),
            "set-aside location is out of play"
        );
    }

    #[test]
    fn moving_to_an_unrevealed_location_reveals_it_and_places_clues() {
        use crate::card_data::ClueValue;
        // 1 investigator; destination `b` is in play but unrevealed with a
        // per-investigator clue value. Entering reveals it and places clues.
        let (inv_id, _a, b, mut state) = move_scenario();
        let loc_b = state.locations.get_mut(&b).unwrap();
        loc_b.revealed = false;
        loc_b.clues = 0;
        loc_b.printed_clues = ClueValue::PerInvestigator(2);
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        let loc_b = &result.state.locations[&b];
        assert!(loc_b.revealed, "entering an unrevealed location reveals it");
        assert_eq!(loc_b.clues, 2, "1 investigator × 2 per-investigator");
    }

    #[test]
    fn investigate_on_an_unrevealed_location_is_rejected() {
        // An unrevealed location is not yet investigatable (Rules Reference
        // p.14). In practice unreachable (entering reveals), but the gate
        // makes the rule explicit.
        let (inv_id, loc_id, mut state) = investigate_scenario(2, 2);
        state.locations.get_mut(&loc_id).unwrap().revealed = false;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
        assert!(
            matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "unrevealed location cannot be investigated"
        );
    }

    #[test]
    #[should_panic(expected = "state-corruption invariant violation")]
    fn investigate_with_active_investigator_missing_from_map_panics() {
        // Same corruption pattern as above, applied to Investigate.
        let (inv_id, _, mut state) = investigate_scenario(2, 2);
        state.investigators.remove(&inv_id);
        let _ = apply(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
    }

    /// Build a scenario suitable for Fight/Evade tests: one
    /// investigator engaged with one enemy. Bag is `Numeric(0)` so
    /// the test outcome is determined purely by (skill vs
    /// fight/evade). Investigation phase, investigator is active,
    /// 3 actions. Returns (investigator id, enemy id, state).
    fn fight_evade_scenario() -> (InvestigatorId, EnemyId, GameState) {
        let inv_id = InvestigatorId(1);
        let enemy_id = EnemyId(100);
        let mut inv = test_investigator(1);
        inv.actions_remaining = 3;
        let mut enemy = test_enemy(100, "Test Ghoul");
        enemy.fight = 3;
        enemy.evade = 3;
        enemy.max_health = 2;
        enemy.engaged_with = Some(inv_id);
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_enemy(enemy)
            .with_chaos_bag(bag_only_zero())
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv_id)
            .build();
        (inv_id, enemy_id, state)
    }

    #[test]
    fn fight_succeeds_deals_one_damage_and_spends_action() {
        // Combat 3, fight 3, modifier 0 → margin 0 → success.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        // Set investigator combat = 3 (default already is 3) so the
        // test just barely passes.
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 3;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::SkillTestStarted {
                skill: SkillKind::Combat,
                difficulty: 3,
                ..
            }
        );
        assert_event!(result.events, Event::SkillTestSucceeded { .. });
        assert_event!(
            result.events,
            Event::EnemyDamaged { enemy: e, amount: 1, new_damage: 1 } if *e == enemy_id
        );
        assert_no_event!(result.events, Event::EnemyDefeated { .. });
        assert_eq!(result.state.enemies[&enemy_id].damage, 1);
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    }

    #[test]
    fn fight_failure_spends_action_but_deals_no_damage() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::EnemyDamaged { .. });
        assert_no_event!(result.events, Event::EnemyDefeated { .. });
        assert_eq!(result.state.enemies[&enemy_id].damage, 0);
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    }

    #[test]
    fn failed_fight_against_ready_retaliate_enemy_triggers_attack() {
        // Combat 1 vs fight 3 → fail. Enemy retaliates 1 dmg + 1 horror.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::SkillTestFailed { .. });
        // Retaliate attack lands (damage + horror, simultaneously).
        assert_event!(result.events, Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id);
        assert_event!(result.events, Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id);
        assert_eq!(result.state.investigators[&inv_id].damage, 1);
        assert_eq!(result.state.investigators[&inv_id].horror, 1);
        // Enemy does NOT exhaust after a retaliate attack (RR p.18).
        assert!(!result.state.enemies[&enemy_id].exhausted);
        // Failed fight dealt no damage to the enemy.
        assert_no_event!(result.events, Event::EnemyDamaged { .. });
        // Skill test still tears down.
        assert_event!(result.events, Event::SkillTestEnded { .. });
    }

    #[test]
    fn successful_fight_against_retaliate_enemy_does_not_trigger_attack() {
        // Combat 3 vs fight 3 → success; retaliate must NOT fire.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 3;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestSucceeded { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_no_event!(result.events, Event::HorrorTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn failed_fight_against_exhausted_retaliate_enemy_does_not_trigger_attack() {
        // Retaliate requires a READY enemy (RR p.18). Exhausted → no attack.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.exhausted = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn failed_fight_against_non_retaliate_enemy_does_not_trigger_attack() {
        // No retaliate flag → no attack on failure.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.combat = 1;
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn failed_evade_against_retaliate_enemy_does_not_trigger_attack() {
        // Retaliate is "while attacking" — a failed Evade must NOT fire it.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.agility = 1; // vs evade 3 → fail
        let e = state.enemies.get_mut(&enemy_id).unwrap();
        e.retaliate = true;
        e.attack_damage = 1;
        e.attack_horror = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn fight_defeats_enemy_when_damage_reaches_max_health() {
        // Enemy at 1/2 already; Fight success → damage 2, defeated,
        // removed from state, engagement cleared.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.enemies.get_mut(&enemy_id).unwrap().damage = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::EnemyDamaged { enemy: e, amount: 1, new_damage: 2 } if *e == enemy_id
        );
        assert_event!(
            result.events,
            Event::EnemyDefeated { enemy: e, by: Some(by) }
                if *e == enemy_id && *by == inv_id
        );
        assert!(!result.state.enemies.contains_key(&enemy_id));
    }

    #[test]
    fn evade_succeeds_disengages_and_exhausts() {
        // Default agility 3, evade 3 → margin 0 → success.
        let (inv_id, enemy_id, state) = fight_evade_scenario();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::SkillTestStarted {
                skill: SkillKind::Agility,
                difficulty: 3,
                ..
            }
        );
        assert_event!(result.events, Event::SkillTestSucceeded { .. });
        assert_event!(
            result.events,
            Event::EnemyDisengaged { enemy: e, investigator: i }
                if *e == enemy_id && *i == inv_id
        );
        assert_event!(
            result.events,
            Event::EnemyExhausted { enemy: e } if *e == enemy_id
        );
        assert_eq!(result.state.enemies[&enemy_id].engaged_with, None);
        assert!(result.state.enemies[&enemy_id].exhausted);
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
    }

    #[test]
    fn evade_failure_leaves_engagement_intact() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().skills.agility = 1;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::SkillTestFailed { .. });
        assert_no_event!(result.events, Event::EnemyDisengaged { .. });
        assert_no_event!(result.events, Event::EnemyExhausted { .. });
        assert_eq!(result.state.enemies[&enemy_id].engaged_with, Some(inv_id));
        assert!(!result.state.enemies[&enemy_id].exhausted);
    }

    #[test]
    fn fight_when_not_engaged_with_target_is_rejected() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.enemies.get_mut(&enemy_id).unwrap().engaged_with = None;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn evade_when_not_engaged_with_target_is_rejected() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.enemies.get_mut(&enemy_id).unwrap().engaged_with = None;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn fight_with_unknown_enemy_is_rejected() {
        let (inv_id, _, state) = fight_evade_scenario();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: EnemyId(9999),
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn fight_outside_investigation_phase_is_rejected() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.phase = Phase::Mythos;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn fight_by_non_active_investigator_is_rejected() {
        let (_, enemy_id, mut state) = fight_evade_scenario();
        let other = InvestigatorId(2);
        state.investigators.insert(other, test_investigator(2));
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: other,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn fight_with_zero_actions_is_rejected() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state
            .investigators
            .get_mut(&inv_id)
            .unwrap()
            .actions_remaining = 0;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn fight_with_negative_fight_value_is_rejected_without_mutating_state() {
        // Malformed scenario data: fight = -1. validate-first must
        // reject BEFORE spend_one_action runs, otherwise the action
        // is silently lost without a rejection event.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.enemies.get_mut(&enemy_id).unwrap().fight = -1;
        let actions_before = state.investigators[&inv_id].actions_remaining;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
        assert_eq!(
            result.state.investigators[&inv_id].actions_remaining,
            actions_before
        );
    }

    #[test]
    fn evade_with_negative_evade_value_is_rejected_without_mutating_state() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.enemies.get_mut(&enemy_id).unwrap().evade = -1;
        let actions_before = state.investigators[&inv_id].actions_remaining;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
        assert_eq!(
            result.state.investigators[&inv_id].actions_remaining,
            actions_before
        );
    }

    #[test]
    fn evade_on_already_exhausted_enemy_is_idempotent_on_exhaust() {
        // Edge: enemy is already exhausted but still engaged (e.g.
        // attacked the investigator earlier this round, now the
        // investigator Evades). Success disengages and leaves
        // `exhausted = true`.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.enemies.get_mut(&enemy_id).unwrap().exhausted = true;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::SkillTestSucceeded { .. });
        assert_event!(result.events, Event::EnemyDisengaged { .. });
        assert!(result.state.enemies[&enemy_id].exhausted);
        assert_eq!(result.state.enemies[&enemy_id].engaged_with, None);
    }

    #[test]
    fn fight_engaged_with_two_enemies_only_touches_the_target() {
        // Investigator engaged with two enemies. Fight one. The other
        // engagement must stay intact and its state untouched.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        let other_id = EnemyId(101);
        let mut other = test_enemy(101, "Bystander Ghoul");
        other.engaged_with = Some(inv_id);
        state.enemies.insert(other_id, other);
        // Make sure the Fight defeats the target so we observe the
        // full attribution + removal path while the other is untouched.
        state.enemies.get_mut(&enemy_id).unwrap().damage = 1;

        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert!(!result.state.enemies.contains_key(&enemy_id));
        // Other enemy untouched.
        assert!(result.state.enemies.contains_key(&other_id));
        let other_after = &result.state.enemies[&other_id];
        assert_eq!(other_after.engaged_with, Some(inv_id));
        assert_eq!(other_after.damage, 0);
        assert!(!other_after.exhausted);
    }

    // ------------------------------------------------------------------
    // Attack-of-opportunity tests (#78)
    // ------------------------------------------------------------------

    /// Move scenario with a ready enemy engaged with the active
    /// investigator at the origin. A connects to B (one-way).
    /// Returns (inv id, A, B, enemy id, state).
    fn move_scenario_with_engaged_enemy() -> (
        InvestigatorId,
        crate::state::LocationId,
        crate::state::LocationId,
        EnemyId,
        GameState,
    ) {
        let (inv_id, a, b, mut state) = move_scenario();
        let enemy_id = EnemyId(200);
        let mut enemy = test_enemy(200, "Engaged Ghoul");
        enemy.current_location = Some(a);
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 1;
        enemy.attack_horror = 0;
        state.enemies.insert(enemy_id, enemy);
        (inv_id, a, b, enemy_id, state)
    }

    #[test]
    fn move_with_ready_engaged_enemy_fires_aoo_and_enemy_follows() {
        let (inv_id, a, b, enemy_id, state) = move_scenario_with_engaged_enemy();
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        );
        // AoO damage must fire BEFORE the move resolves per the Rules
        // Reference. assert_event! is existence-only, so check the
        // positions explicitly.
        let damage_idx = result
            .events
            .iter()
            .position(|e| matches!(e, Event::DamageTaken { .. }))
            .expect("DamageTaken event missing");
        let moved_idx = result
            .events
            .iter()
            .position(|e| matches!(e, Event::InvestigatorMoved { .. }))
            .expect("InvestigatorMoved event missing");
        assert!(
            damage_idx < moved_idx,
            "AoO DamageTaken (idx {damage_idx}) must precede InvestigatorMoved (idx {moved_idx})"
        );
        // Investigator damaged.
        assert_eq!(result.state.investigators[&inv_id].damage, 1);
        // Investigator moved.
        assert_eq!(
            result.state.investigators[&inv_id].current_location,
            Some(b)
        );
        assert_event!(
            result.events,
            Event::InvestigatorMoved { from, to, .. } if *from == a && *to == b
        );
        // Engaged enemy followed.
        assert_eq!(result.state.enemies[&enemy_id].current_location, Some(b));
        assert_eq!(result.state.enemies[&enemy_id].engaged_with, Some(inv_id));
        // AoO does NOT exhaust per the Rules Reference.
        assert!(!result.state.enemies[&enemy_id].exhausted);
    }

    #[test]
    fn move_with_exhausted_engaged_enemy_does_not_fire_aoo() {
        let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
        state.enemies.get_mut(&enemy_id).unwrap().exhausted = true;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_no_event!(result.events, Event::HorrorTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
        // Exhausted enemy still follows the investigator.
        assert_eq!(result.state.enemies[&enemy_id].current_location, Some(b));
    }

    #[test]
    fn move_with_unengaged_enemy_at_origin_leaves_enemy_behind() {
        let (inv_id, a, b, _, mut state) = move_scenario_with_engaged_enemy();
        // Convert the engagement into a non-engagement: enemy is at A
        // but not engaged with anyone.
        let other_id = EnemyId(201);
        let mut other = test_enemy(201, "Bystander");
        other.current_location = Some(a);
        // engaged_with stays None.
        state.enemies.insert(other_id, other);
        // Remove the engaged enemy so the move doesn't trigger AoO,
        // keeping the focus on the unengaged enemy.
        state.enemies.remove(&EnemyId(200));

        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        // Investigator moved.
        assert_eq!(
            result.state.investigators[&inv_id].current_location,
            Some(b)
        );
        // Unengaged enemy stayed put.
        assert_eq!(result.state.enemies[&other_id].current_location, Some(a));
    }

    #[test]
    fn investigate_with_ready_engaged_enemy_fires_aoo() {
        // Set up an Investigate scenario, then attach an engaged
        // enemy at the investigator's location.
        let (inv_id, loc_id, state) = investigate_scenario(2, 2);
        let enemy_id = EnemyId(300);
        let mut enemy = test_enemy(300, "Engaged at Study");
        enemy.current_location = Some(loc_id);
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 0;
        enemy.attack_horror = 1;
        let mut state = state;
        state.enemies.insert(enemy_id, enemy);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id
        );
        // Skill test still runs after AoO.
        assert_event!(result.events, Event::SkillTestStarted { .. });
        assert_eq!(result.state.investigators[&inv_id].horror, 1);
    }

    #[test]
    fn fight_does_not_fire_aoo_from_other_engaged_enemy() {
        // Investigator engaged with the Fight target AND a second
        // ready engaged enemy. Fight is on the AoO-exempt list, so
        // no AoO fires — neither from the target nor from the
        // bystander.
        let (inv_id, target_id, mut state) = fight_evade_scenario();
        let bystander_id = EnemyId(202);
        let mut bystander = test_enemy(202, "Other Ghoul");
        bystander.engaged_with = Some(inv_id);
        bystander.attack_damage = 5;
        state.enemies.insert(bystander_id, bystander);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: target_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_no_event!(result.events, Event::HorrorTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
        assert_eq!(result.state.investigators[&inv_id].horror, 0);
    }

    #[test]
    fn evade_does_not_fire_aoo_from_other_engaged_enemy() {
        let (inv_id, target_id, mut state) = fight_evade_scenario();
        let bystander_id = EnemyId(203);
        let mut bystander = test_enemy(203, "Other Ghoul");
        bystander.engaged_with = Some(inv_id);
        bystander.attack_damage = 5;
        state.enemies.insert(bystander_id, bystander);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: target_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_no_event!(result.events, Event::HorrorTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
    }

    #[test]
    fn move_with_no_engaged_enemy_does_not_fire_aoo() {
        // Regression: the AoO step is a no-op when no engaged
        // enemies exist; pre-existing Move tests should not have
        // started failing.
        let (inv_id, _, b, state) = move_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::DamageTaken { .. });
    }

    #[test]
    fn aoo_fires_in_enemy_id_order_for_multiple_attackers() {
        // Lock in the v1 ordering contract (deterministic by EnemyId
        // via BTreeMap iteration). Three engaged ready enemies with
        // distinct attack_damage values; the sequence of DamageTaken
        // amounts must match EnemyId ordering.
        let (inv_id, _, b, state) = move_scenario();
        let mut state = state;
        for (id, dmg) in [(300, 1), (301, 2), (302, 4)] {
            let mut e = test_enemy(id, "");
            e.engaged_with = Some(inv_id);
            e.attack_damage = dmg;
            state.enemies.insert(EnemyId(id), e);
        }
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        let damages: Vec<u8> = result
            .events
            .iter()
            .filter_map(|e| match e {
                Event::DamageTaken { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect();
        assert_eq!(damages, vec![1, 2, 4]);
        assert_eq!(result.state.investigators[&inv_id].damage, 7);
    }

    #[test]
    fn aoo_from_zero_damage_zero_horror_enemy_emits_no_events() {
        // Edge: an engaged ready enemy with attack_damage = 0 and
        // attack_horror = 0 still "attacks" but the helper's `if > 0`
        // guards must skip both event emissions.
        let (inv_id, _, b, state) = move_scenario();
        let mut state = state;
        let mut e = test_enemy(310, "Quiet Watcher");
        e.engaged_with = Some(inv_id);
        e.attack_damage = 0;
        e.attack_horror = 0;
        state.enemies.insert(EnemyId(310), e);
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_no_event!(result.events, Event::DamageTaken { .. });
        assert_no_event!(result.events, Event::HorrorTaken { .. });
        assert_eq!(result.state.investigators[&inv_id].damage, 0);
        assert_eq!(result.state.investigators[&inv_id].horror, 0);
    }

    // ------------------------------------------------------------------
    // Investigator-defeat tests (#80)
    // ------------------------------------------------------------------

    /// Build a Move scenario with one ready engaged enemy at the
    /// origin. The investigator is configured to be defeated by the
    /// enemy's `AoO`: `max_health = 1`, `damage = 0`, enemy
    /// `attack_damage = 1`. Returns (inv id, origin, dest, enemy id,
    /// state).
    fn move_scenario_with_lethal_aoo() -> (
        InvestigatorId,
        crate::state::LocationId,
        crate::state::LocationId,
        EnemyId,
        GameState,
    ) {
        let (inv_id, a, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
        state.investigators.get_mut(&inv_id).unwrap().max_health = 1;
        // attack_damage = 1 is already the default from
        // move_scenario_with_engaged_enemy, but be explicit.
        state.enemies.get_mut(&enemy_id).unwrap().attack_damage = 1;
        (inv_id, a, b, enemy_id, state)
    }

    #[test]
    fn aoo_lethal_damage_defeats_investigator_during_move_and_cancels_move() {
        let (inv_id, a, b, enemy_id, state) = move_scenario_with_lethal_aoo();
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        // Damage applied + defeat event fired.
        assert_event!(
            result.events,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::InvestigatorDefeated {
                investigator,
                cause: DefeatCause::Damage,
            } if *investigator == inv_id
        );
        // Status flipped to Killed.
        assert_eq!(result.state.investigators[&inv_id].status, Status::Killed);
        // Action point still spent (the action declaration stays).
        assert_eq!(result.state.investigators[&inv_id].actions_remaining, 2);
        // Move suppressed: no InvestigatorMoved event; enemy stays at
        // the origin. The investigator's location is cleared to None
        // (elimination step 3 — they have left play).
        assert_no_event!(result.events, Event::InvestigatorMoved { .. });
        assert_eq!(
            result.state.investigators[&inv_id].current_location, None,
            "eliminated investigator has no location (left play)"
        );
        assert_eq!(result.state.enemies[&enemy_id].current_location, Some(a));
        // Single-investigator scenario, so AllInvestigatorsDefeated
        // also fires.
        assert_event!(result.events, Event::AllInvestigatorsDefeated);
    }

    #[test]
    fn aoo_lethal_horror_defeats_investigator_during_investigate_and_cancels_test() {
        // Set up Investigate with an engaged enemy whose attack is
        // pure horror. Investigator's max_sanity = 1, so 1 horror
        // drives them insane.
        let (inv_id, loc_id, mut state) = investigate_scenario(2, 2);
        state.investigators.get_mut(&inv_id).unwrap().max_sanity = 1;
        let enemy_id = EnemyId(400);
        let mut enemy = test_enemy(400, "Tormenting Shade");
        enemy.current_location = Some(loc_id);
        enemy.engaged_with = Some(inv_id);
        enemy.attack_damage = 0;
        enemy.attack_horror = 1;
        state.enemies.insert(enemy_id, enemy);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::InvestigatorDefeated {
                investigator,
                cause: DefeatCause::Horror,
            } if *investigator == inv_id
        );
        assert_eq!(result.state.investigators[&inv_id].status, Status::Insane);
        // Skill test suppressed: no SkillTestStarted event.
        assert_no_event!(result.events, Event::SkillTestStarted { .. });
        assert_no_event!(result.events, Event::CluePlaced { .. });
    }

    #[test]
    fn aoo_damage_to_active_investigator_below_threshold_does_not_defeat() {
        // Sanity check: AoO that doesn't reach max_health leaves the
        // investigator Active. Same as the existing AoO test but
        // explicit on the status field and absence of defeat events.
        let (inv_id, _, b, _, mut state) = move_scenario_with_engaged_enemy();
        // Bump max_health above the AoO damage (which is 1).
        state.investigators.get_mut(&inv_id).unwrap().max_health = 5;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.investigators[&inv_id].status, Status::Active);
        assert_no_event!(result.events, Event::InvestigatorDefeated { .. });
        // Move proceeds.
        assert_event!(result.events, Event::InvestigatorMoved { .. });
    }

    #[test]
    fn defeated_investigator_does_not_take_further_damage() {
        // Two engaged ready enemies, both with attack_damage = 5.
        // Investigator has max_health = 1. The first AoO defeats;
        // the second is a no-op (apply_damage_numeric skips defeated).
        let (inv_id, _, b, _, mut state) = move_scenario_with_engaged_enemy();
        state.investigators.get_mut(&inv_id).unwrap().max_health = 1;
        state.enemies.get_mut(&EnemyId(200)).unwrap().attack_damage = 5;
        // Add a second engaged ready enemy.
        let mut e2 = test_enemy(201, "Second Ghoul");
        e2.engaged_with = Some(inv_id);
        e2.attack_damage = 5;
        state.enemies.insert(EnemyId(201), e2);
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        // Exactly one DamageTaken event (from the first AoO) and one
        // InvestigatorDefeated.
        assert_event_count!(result.events, 1, Event::DamageTaken { .. });
        assert_event_count!(result.events, 1, Event::InvestigatorDefeated { .. });
        // Damage saturates at the first AoO's amount; second AoO is
        // skipped entirely.
        assert_eq!(result.state.investigators[&inv_id].damage, 5);
    }

    #[test]
    fn aoo_with_lethal_damage_and_sublethal_horror_applies_both_numerically() {
        // Rules Reference page 7: damage and horror from a single
        // attack are applied simultaneously. Lethal damage MUST NOT
        // short-circuit the horror application.
        let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
        state.investigators.get_mut(&inv_id).unwrap().max_health = 5;
        // Plenty of sanity headroom so the horror is sub-lethal.
        state.investigators.get_mut(&inv_id).unwrap().max_sanity = 8;
        let enemy = state.enemies.get_mut(&enemy_id).unwrap();
        enemy.attack_damage = 5;
        enemy.attack_horror = 1;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        // Both events fire and both numeric fields land.
        assert_event!(
            result.events,
            Event::DamageTaken { investigator, amount: 5 } if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == inv_id
        );
        assert_eq!(result.state.investigators[&inv_id].damage, 5);
        assert_eq!(result.state.investigators[&inv_id].horror, 1);
        // Exactly one InvestigatorDefeated, caused by Damage.
        assert_event_count!(result.events, 1, Event::InvestigatorDefeated { .. });
        assert_event!(
            result.events,
            Event::InvestigatorDefeated {
                investigator,
                cause: DefeatCause::Damage,
            } if *investigator == inv_id
        );
        assert_eq!(result.state.investigators[&inv_id].status, Status::Killed);
    }

    #[test]
    fn aoo_with_sublethal_damage_and_lethal_horror_applies_both_numerically() {
        // Symmetric to the lethal-damage case: lethal horror MUST NOT
        // short-circuit the damage application.
        let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
        state.investigators.get_mut(&inv_id).unwrap().max_health = 8;
        state.investigators.get_mut(&inv_id).unwrap().max_sanity = 1;
        let enemy = state.enemies.get_mut(&enemy_id).unwrap();
        enemy.attack_damage = 1;
        enemy.attack_horror = 5;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        );
        assert_event!(
            result.events,
            Event::HorrorTaken { investigator, amount: 5 } if *investigator == inv_id
        );
        assert_eq!(result.state.investigators[&inv_id].damage, 1);
        assert_eq!(result.state.investigators[&inv_id].horror, 5);
        assert_event_count!(result.events, 1, Event::InvestigatorDefeated { .. });
        assert_event!(
            result.events,
            Event::InvestigatorDefeated {
                investigator,
                cause: DefeatCause::Horror,
            } if *investigator == inv_id
        );
        assert_eq!(result.state.investigators[&inv_id].status, Status::Insane);
    }

    #[test]
    fn aoo_with_both_lethal_defeats_once_with_damage_cause() {
        // Both stats cross their threshold from the same attack. Per
        // the enemy_attack doc comment, the tie-break is
        // DefeatCause::Damage (Rules Reference is silent on the
        // simultaneous-lethal case; damage-first is the convention).
        let (inv_id, _, b, enemy_id, mut state) = move_scenario_with_engaged_enemy();
        state.investigators.get_mut(&inv_id).unwrap().max_health = 1;
        state.investigators.get_mut(&inv_id).unwrap().max_sanity = 1;
        let enemy = state.enemies.get_mut(&enemy_id).unwrap();
        enemy.attack_damage = 1;
        enemy.attack_horror = 1;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.investigators[&inv_id].damage, 1);
        assert_eq!(result.state.investigators[&inv_id].horror, 1);
        assert_event_count!(result.events, 1, Event::InvestigatorDefeated { .. });
        assert_event!(
            result.events,
            Event::InvestigatorDefeated {
                investigator,
                cause: DefeatCause::Damage,
            } if *investigator == inv_id
        );
        assert_eq!(result.state.investigators[&inv_id].status, Status::Killed);
    }

    #[test]
    fn all_investigators_defeated_fires_only_when_last_active_falls() {
        // Two investigators, one defeated, then the second defeated.
        // AllInvestigatorsDefeated should fire only on the second.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut i1 = test_investigator(1);
        i1.max_health = 1;
        i1.actions_remaining = 3;
        let i2 = test_investigator(2);
        // i2 stays at default 8/8.
        let mut e = test_enemy(500, "Lethal Ghoul");
        e.engaged_with = Some(inv1);
        e.attack_damage = 1;
        let a = crate::state::LocationId(10);
        let b = crate::state::LocationId(11);
        let mut loc_a = test_location(10, "A");
        loc_a.connections = vec![b];
        let state = GameStateBuilder::new()
            .with_investigator(i1)
            .with_investigator(i2)
            .with_location(loc_a)
            .with_location(test_location(11, "B"))
            .with_chaos_bag(bag_only_zero())
            .with_phase(Phase::Investigation)
            .with_active_investigator(inv1)
            .with_enemy(e)
            .build();
        // First, place inv1 at A so the move scenario validates.
        let mut state = state;
        state.investigators.get_mut(&inv1).unwrap().current_location = Some(a);

        // inv1 moves → AoO defeats them. inv2 is still Active.
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv1,
                destination: b,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::InvestigatorDefeated { investigator, .. } if *investigator == inv1
        );
        assert_no_event!(result.events, Event::AllInvestigatorsDefeated);
        assert_eq!(result.state.investigators[&inv1].status, Status::Killed);
        assert_eq!(result.state.investigators[&inv2].status, Status::Active);
    }

    #[test]
    fn defeated_investigator_cannot_move() {
        let (inv_id, _, b, _, mut state) = move_scenario_with_engaged_enemy();
        state.investigators.get_mut(&inv_id).unwrap().status = Status::Killed;
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: inv_id,
                destination: b,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn defeated_investigator_cannot_investigate() {
        let (inv_id, _, mut state) = investigate_scenario(2, 2);
        state.investigators.get_mut(&inv_id).unwrap().status = Status::Insane;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn defeated_investigator_cannot_fight() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().status = Status::Killed;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Fight {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn defeated_investigator_cannot_evade() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().status = Status::Insane;
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Evade {
                investigator: inv_id,
                enemy: enemy_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn defeated_investigator_cannot_perform_skill_test() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.status = Status::Killed;
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Willpower,
                difficulty: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    // ------------------------------------------------------------------
    // Draw tests (#84)
    // ------------------------------------------------------------------

    /// Build a Draw scenario: one investigator at A, in Investigation
    /// phase, active, 3 actions. The caller mutates deck/hand/discard
    /// before the test.
    fn draw_scenario() -> (InvestigatorId, GameState) {
        let id = InvestigatorId(1);
        let a = crate::state::LocationId(10);
        let mut inv = test_investigator(1);
        inv.current_location = Some(a);
        inv.actions_remaining = 3;
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(test_location(10, "A"))
            .with_phase(Phase::Investigation)
            .with_active_investigator(id)
            .with_rng_seed(13)
            .build();
        (id, state)
    }

    #[test]
    fn draw_with_non_empty_deck_draws_one_and_spends_action() {
        let (id, mut state) = draw_scenario();
        state.investigators.get_mut(&id).unwrap().deck =
            vec![CardCode::new("test-001"), CardCode::new("test-002")];
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw { investigator: id }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 2 }
                if *investigator == id
        );
        assert_event!(
            result.events,
            Event::CardsDrawn { investigator, count: 1 } if *investigator == id
        );
        assert_no_event!(result.events, Event::DeckShuffled { .. });
        let inv = &result.state.investigators[&id];
        assert_eq!(inv.hand.len(), 1);
        assert_eq!(inv.deck.len(), 1);
        assert_eq!(inv.hand[0], CardCode::new("test-001"));
        assert_eq!(inv.actions_remaining, 2);
    }

    #[test]
    fn draw_with_empty_deck_reshuffles_discard_draws_and_takes_one_horror() {
        // Per the Rules Reference: empty deck on draw triggers
        // reshuffle, then draw, then 1 horror penalty.
        let (id, mut state) = draw_scenario();
        state.investigators.get_mut(&id).unwrap().discard = vec![
            CardCode::new("test-A"),
            CardCode::new("test-B"),
            CardCode::new("test-C"),
        ];
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw { investigator: id }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::DeckShuffled { investigator } if *investigator == id
        );
        assert_event!(
            result.events,
            Event::CardsDrawn { investigator, count: 1 } if *investigator == id
        );
        assert_event!(
            result.events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == id
        );
        let shuffle_idx = result
            .events
            .iter()
            .position(|e| matches!(e, Event::DeckShuffled { .. }))
            .expect("DeckShuffled missing");
        let draw_idx = result
            .events
            .iter()
            .position(|e| matches!(e, Event::CardsDrawn { .. }))
            .expect("CardsDrawn missing");
        let horror_idx = result
            .events
            .iter()
            .position(|e| matches!(e, Event::HorrorTaken { .. }))
            .expect("HorrorTaken missing");
        assert!(
            shuffle_idx < draw_idx && draw_idx < horror_idx,
            "Expected order DeckShuffled ({shuffle_idx}) < CardsDrawn ({draw_idx}) < \
             HorrorTaken ({horror_idx})"
        );
        let inv = &result.state.investigators[&id];
        assert_eq!(inv.hand.len(), 1);
        assert_eq!(inv.deck.len(), 2);
        assert!(inv.discard.is_empty());
        assert_eq!(inv.horror, 1);
    }

    #[test]
    fn draw_with_both_empty_deals_one_horror_no_shuffle_no_card() {
        // Per the Rules Reference: empty discard means no shuffle
        // happens. We still apply the 1-horror penalty as the safer
        // reading of "would-draw-from-empty-deck" (see handler doc).
        let (id, state) = draw_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw { investigator: id }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == id
        );
        assert_event!(
            result.events,
            Event::CardsDrawn { investigator, count: 0 } if *investigator == id
        );
        assert_no_event!(result.events, Event::DeckShuffled { .. });
        let inv = &result.state.investigators[&id];
        assert_eq!(inv.horror, 1);
        assert_eq!(inv.actions_remaining, 2);
        assert!(inv.hand.is_empty());
        assert!(inv.deck.is_empty());
        assert!(inv.discard.is_empty());
    }

    #[test]
    fn draw_outside_investigation_phase_is_rejected() {
        let (id, mut state) = draw_scenario();
        state.phase = Phase::Mythos;
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw { investigator: id }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn draw_by_non_active_investigator_is_rejected() {
        let (_, mut state) = draw_scenario();
        let other = InvestigatorId(2);
        state.investigators.insert(other, test_investigator(2));
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw {
                investigator: other,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn draw_with_zero_actions_is_rejected() {
        let (id, mut state) = draw_scenario();
        state.investigators.get_mut(&id).unwrap().actions_remaining = 0;
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw { investigator: id }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn draw_by_defeated_investigator_is_rejected() {
        let (id, mut state) = draw_scenario();
        state.investigators.get_mut(&id).unwrap().status = Status::Killed;
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw { investigator: id }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn draw_with_low_sanity_investigator_defeated_by_reshuffle_horror() {
        // Interaction: 1-sanity investigator + empty deck + empty
        // discard → the 1-horror penalty defeats the investigator
        // via take_horror's defeat path (#80). Verifies the Draw
        // flow correctly composes with the defeat helpers.
        let (id, mut state) = draw_scenario();
        let inv = state.investigators.get_mut(&id).unwrap();
        inv.max_sanity = 1;
        let result = apply(
            state,
            Action::Player(PlayerAction::Draw { investigator: id }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == id
        );
        assert_event!(
            result.events,
            Event::InvestigatorDefeated {
                investigator,
                cause: DefeatCause::Horror,
            } if *investigator == id
        );
        assert_eq!(result.state.investigators[&id].status, Status::Insane);
    }

    // ------------------------------------------------------------------
    // Mulligan tests (#85)
    // ------------------------------------------------------------------

    /// Build a Mulligan scenario: one investigator with a known hand
    /// of 5 cards + a remaining deck of 5, mulligan cursor seeded.
    /// Bypasses `StartScenario` so tests can control the exact hand
    /// composition.
    fn mulligan_scenario() -> (InvestigatorId, GameState) {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![
            CardCode::new("h-0"),
            CardCode::new("h-1"),
            CardCode::new("h-2"),
            CardCode::new("h-3"),
            CardCode::new("h-4"),
        ];
        inv.deck = vec![
            CardCode::new("d-0"),
            CardCode::new("d-1"),
            CardCode::new("d-2"),
            CardCode::new("d-3"),
            CardCode::new("d-4"),
        ];
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_rng_seed(2026)
            .with_turn_order([id])
            .with_mulligan_pending(id)
            .build();
        (id, state)
    }

    #[test]
    fn mulligan_redraw_subset_swaps_named_cards() {
        // Redraw indices [1, 3] → those two move to deck, deck
        // shuffles, two new cards come back.
        let (id, state) = mulligan_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![1, 3],
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::MulliganPerformed { investigator, redrawn_count: 2 }
                if *investigator == id
        );
        let inv = &result.state.investigators[&id];
        assert_eq!(inv.hand.len(), 5);
        assert_eq!(inv.deck.len(), 5);
        // h-0, h-2, h-4 stay at relative positions 0/1/2 of the hand
        // (since 1 and 3 got removed, the survivors are in original
        // order at positions 0/1/2). The last 2 hand slots are new
        // draws.
        assert_eq!(inv.hand[0], CardCode::new("h-0"));
        assert_eq!(inv.hand[1], CardCode::new("h-2"));
        assert_eq!(inv.hand[2], CardCode::new("h-4"));
    }

    #[test]
    fn mulligan_redraw_none_keeps_hand_and_consumes_one_shot() {
        let (id, state) = mulligan_scenario();
        let original_hand = state.investigators[&id].hand.clone();
        let original_deck = state.investigators[&id].deck.clone();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::MulliganPerformed { investigator, redrawn_count: 0 }
                if *investigator == id
        );
        let inv = &result.state.investigators[&id];
        // Hand unchanged.
        assert_eq!(inv.hand, original_hand);
        // Deck unchanged (no shuffle happens when nothing moves into it).
        assert_eq!(inv.deck, original_deck);
        // No DeckShuffled (deck wasn't touched).
        assert_no_event!(result.events, Event::DeckShuffled { .. });
    }

    #[test]
    fn mulligan_redraw_all_replaces_entire_hand() {
        let (id, state) = mulligan_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![0, 1, 2, 3, 4],
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::MulliganPerformed { investigator, redrawn_count: 5 }
                if *investigator == id
        );
        let inv = &result.state.investigators[&id];
        assert_eq!(inv.hand.len(), 5);
        assert_eq!(inv.deck.len(), 5);
        // None of the original hand cards are in the new hand —
        // because all 5 went to deck and the shuffle + redraw could
        // theoretically reproduce the same hand by chance, but
        // verify that the hand-as-set is a subset of the union.
        // Stronger check: hand + deck (multiset) equals the original
        // hand + deck (multiset).
        let mut all: Vec<_> = inv.hand.iter().chain(inv.deck.iter()).cloned().collect();
        all.sort();
        let mut expected: Vec<CardCode> = [
            "h-0", "h-1", "h-2", "h-3", "h-4", "d-0", "d-1", "d-2", "d-3", "d-4",
        ]
        .iter()
        .map(|s| CardCode::new(*s))
        .collect();
        expected.sort();
        assert_eq!(all, expected);
    }

    #[test]
    fn mulligan_second_attempt_is_rejected() {
        let (id, state) = mulligan_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![0],
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        // Try again on the post-mulligan state.
        let result2 = apply(
            result.state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![0],
            }),
        );
        assert!(matches!(result2.outcome, EngineOutcome::Rejected { .. }));
        assert!(result2.events.is_empty());
    }

    #[test]
    fn mulligan_after_cursor_cleared_is_rejected() {
        let (id, mut state) = mulligan_scenario();
        state.mulligan_pending = None;
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![0],
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn mulligan_by_defeated_investigator_is_rejected() {
        // The seed/advance helpers skip non-Active investigators, so the
        // cursor never points at a defeated one. With inv1 Killed the
        // cursor sits on inv2; a Mulligan from the defeated inv1 is
        // rejected by the cursor mismatch.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        a.status = Status::Killed;
        let b = test_investigator(2);
        let state = GameStateBuilder::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .with_mulligan_pending(inv2)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn mulligan_with_out_of_bounds_index_is_rejected() {
        let (id, state) = mulligan_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![10],
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn mulligan_with_duplicate_indices_is_rejected() {
        let (id, state) = mulligan_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![1, 1],
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn start_scenario_seeds_mulligan_cursor() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = make_test_deck(10);
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.mulligan_pending, Some(id));
    }

    #[test]
    fn non_mulligan_action_while_mulligan_pending_is_rejected() {
        // Non-Mulligan player actions are gated by the mulligan
        // cursor: the engine refuses Move/Investigate/etc. until
        // every investigator has signaled their mulligan choice.
        let id = InvestigatorId(1);
        let a = crate::state::LocationId(10);
        let b = crate::state::LocationId(11);
        let mut inv = test_investigator(1);
        inv.current_location = Some(a);
        inv.actions_remaining = 3;
        let mut loc_a = test_location(10, "A");
        loc_a.connections = vec![b];
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc_a)
            .with_location(test_location(11, "B"))
            .with_phase(Phase::Investigation)
            .with_active_investigator(id)
            .with_turn_order([id])
            .with_mulligan_pending(id)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::Move {
                investigator: id,
                destination: b,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn solo_mulligan_clears_the_cursor() {
        // Single-investigator scenario: as soon as that one
        // investigator mulligans (empty redraw counts), all
        // investigators have mulliganed and the cursor clears.
        let (id, state) = mulligan_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.mulligan_pending, None);
    }

    #[test]
    fn multi_investigator_mulligan_advances_cursor_in_player_order() {
        // Two investigators; the cursor advances inv1 → inv2 → None as
        // each mulligans in player order.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        let mut b = test_investigator(2);
        a.hand = vec![CardCode::new("a-0")];
        b.hand = vec![CardCode::new("b-0")];
        let state = GameStateBuilder::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .with_mulligan_pending(inv1)
            .build();

        let after_first = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(after_first.outcome, EngineOutcome::Done);
        assert_eq!(after_first.state.mulligan_pending, Some(inv2));

        let after_second = apply(
            after_first.state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(after_second.outcome, EngineOutcome::Done);
        assert_eq!(after_second.state.mulligan_pending, None);
    }

    #[test]
    fn multi_investigator_mulligan_out_of_order_is_rejected() {
        // Cursor is on inv1 (first in turn_order). inv2 trying to
        // mulligan out of turn is rejected, and the cursor is unmoved
        // (Rules Reference p.16: mulligans in player order).
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        let mut b = test_investigator(2);
        a.hand = vec![CardCode::new("a-0")];
        b.hand = vec![CardCode::new("b-0")];
        let state = GameStateBuilder::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .with_mulligan_pending(inv1)
            .build();

        let out_of_order = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        );
        assert!(matches!(
            out_of_order.outcome,
            EngineOutcome::Rejected { .. }
        ));
        assert!(out_of_order.events.is_empty());
        assert_eq!(out_of_order.state.mulligan_pending, Some(inv1));
    }

    #[test]
    fn multi_investigator_real_redraw_plus_empty_mulligan_combo() {
        // One investigator does a real redraw, the other keeps their
        // hand. Both signal Mulligan; window closes after the
        // second.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        let mut b = test_investigator(2);
        a.hand = vec![
            CardCode::new("a-h-0"),
            CardCode::new("a-h-1"),
            CardCode::new("a-h-2"),
        ];
        a.deck = vec![
            CardCode::new("a-d-0"),
            CardCode::new("a-d-1"),
            CardCode::new("a-d-2"),
        ];
        b.hand = vec![CardCode::new("b-h-0"), CardCode::new("b-h-1")];
        b.deck = vec![CardCode::new("b-d-0")];
        let state = GameStateBuilder::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_turn_order([inv1, inv2])
            .with_mulligan_pending(inv1)
            .with_rng_seed(99)
            .build();

        // inv1 redraws indices [0, 2] → those two go to deck, deck
        // shuffles, two new cards come back.
        let after_inv1 = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![0, 2],
            }),
        );
        assert_eq!(after_inv1.outcome, EngineOutcome::Done);
        assert_eq!(after_inv1.state.mulligan_pending, Some(inv2)); // inv2 hasn't yet
        let inv1_after = &after_inv1.state.investigators[&inv1];
        assert_eq!(inv1_after.hand.len(), 3);
        assert_eq!(inv1_after.deck.len(), 3);
        assert_event!(
            after_inv1.events,
            Event::MulliganPerformed { investigator, redrawn_count: 2 }
                if *investigator == inv1
        );

        // inv2 keeps hand (empty redraw). Window now closes.
        let original_inv2_hand = after_inv1.state.investigators[&inv2].hand.clone();
        let original_inv2_deck = after_inv1.state.investigators[&inv2].deck.clone();
        let after_inv2 = apply(
            after_inv1.state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(after_inv2.outcome, EngineOutcome::Done);
        assert_eq!(after_inv2.state.mulligan_pending, None);
        assert_event!(
            after_inv2.events,
            Event::MulliganPerformed { investigator, redrawn_count: 0 }
                if *investigator == inv2
        );
        // inv2's zones untouched by their no-op mulligan.
        let inv2_after = &after_inv2.state.investigators[&inv2];
        assert_eq!(inv2_after.hand, original_inv2_hand);
        assert_eq!(inv2_after.deck, original_inv2_deck);
    }

    // ---- PlayCard rejection tests --------------------------------
    //
    // These exercise the validations that fire *before* the registry
    // lookup. Tests that need real-card metadata / abilities live in
    // crates/cards/tests/play_card.rs — that crate can install the
    // real REGISTRY in its own integration-test process without
    // polluting game-core's OnceLock.

    fn play_card_state(active: bool, hand: Vec<CardCode>) -> (GameState, InvestigatorId) {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = hand;
        let mut builder = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(inv);
        if active {
            builder = builder.with_active_investigator(id);
        }
        (builder.build(), id)
    }

    #[test]
    fn play_card_outside_investigation_phase_is_rejected() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![CardCode::new("01059")];
        let state = GameStateBuilder::new()
            .with_phase(Phase::Mythos)
            .with_investigator(inv)
            .with_active_investigator(id)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::PlayCard {
                investigator: id,
                hand_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
        // Hand untouched.
        assert_eq!(
            result.state.investigators[&id].hand,
            vec![CardCode::new("01059")]
        );
    }

    #[test]
    fn play_card_by_non_active_investigator_is_rejected() {
        let (state, id) = play_card_state(false, vec![CardCode::new("01059")]);
        let result = apply(
            state,
            Action::Player(PlayerAction::PlayCard {
                investigator: id,
                hand_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn play_card_by_defeated_investigator_is_rejected() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![CardCode::new("01059")];
        inv.status = Status::Killed;
        let state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(inv)
            .with_active_investigator(id)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::PlayCard {
                investigator: id,
                hand_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn play_card_with_out_of_bounds_hand_index_is_rejected() {
        let (state, id) = play_card_state(true, vec![CardCode::new("01059")]);
        let result = apply(
            state,
            Action::Player(PlayerAction::PlayCard {
                investigator: id,
                hand_index: 5,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn play_card_with_empty_hand_is_rejected() {
        let (state, id) = play_card_state(true, vec![]);
        let result = apply(
            state,
            Action::Player(PlayerAction::PlayCard {
                investigator: id,
                hand_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    // ---- ActivateAbility rejection tests --------------------------
    //
    // State-side rejection prefix only. The full activation flow
    // (registry → ability dispatch → cost payment → effect) needs
    // a registry, which lives in crates/game-core/tests/activate_ability.rs
    // as a separate integration-test binary.

    use crate::state::CardInstanceId;

    fn activate_ability_state(active: bool) -> (GameState, InvestigatorId, CardInstanceId) {
        let id = InvestigatorId(1);
        let instance_id = CardInstanceId(7);
        let mut inv = test_investigator(1);
        inv.cards_in_play.push(crate::state::CardInPlay::enter_play(
            CardCode::new("01059"),
            instance_id,
        ));
        let mut builder = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(inv);
        if active {
            builder = builder.with_active_investigator(id);
        }
        (builder.build(), id, instance_id)
    }

    #[test]
    fn activate_ability_outside_investigation_phase_is_rejected() {
        let id = InvestigatorId(1);
        let instance_id = CardInstanceId(0);
        let mut inv = test_investigator(1);
        inv.cards_in_play.push(crate::state::CardInPlay::enter_play(
            CardCode::new("01059"),
            instance_id,
        ));
        let state = GameStateBuilder::new()
            .with_phase(Phase::Mythos)
            .with_investigator(inv)
            .with_active_investigator(id)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::ActivateAbility {
                investigator: id,
                instance_id,
                ability_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn activate_ability_by_non_active_investigator_is_rejected() {
        let (state, id, instance_id) = activate_ability_state(false);
        let result = apply(
            state,
            Action::Player(PlayerAction::ActivateAbility {
                investigator: id,
                instance_id,
                ability_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn activate_ability_with_unknown_instance_id_is_rejected() {
        let (state, id, _real_instance) = activate_ability_state(true);
        let result = apply(
            state,
            Action::Player(PlayerAction::ActivateAbility {
                investigator: id,
                instance_id: CardInstanceId(9999),
                ability_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn activate_ability_when_defeated_is_rejected() {
        let id = InvestigatorId(1);
        let instance_id = CardInstanceId(0);
        let mut inv = test_investigator(1);
        inv.status = Status::Killed;
        inv.cards_in_play.push(crate::state::CardInPlay::enter_play(
            CardCode::new("01059"),
            instance_id,
        ));
        let state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(inv)
            .with_active_investigator(id)
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::ActivateAbility {
                investigator: id,
                instance_id,
                ability_index: 0,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    // ---- skill-test commit window (#63) -----------------------------

    #[test]
    fn perform_skill_test_awaits_input_between_started_and_revealed() {
        // Acceptance: AwaitingInput must fire between SkillTestStarted
        // and ChaosTokenRevealed. The first `apply` returns
        // AwaitingInput with only SkillTestStarted on the events list.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );

        assert!(matches!(
            result.outcome,
            EngineOutcome::AwaitingInput { .. }
        ));
        assert_event!(
            result.events,
            Event::SkillTestStarted { investigator, .. } if *investigator == id
        );
        // The chaos token has NOT been drawn yet — that fires on the
        // resume path after the commit response arrives.
        assert_no_event!(result.events, Event::ChaosTokenRevealed { .. });
        assert!(
            result.state.in_flight_skill_test.is_some(),
            "in_flight_skill_test must be populated while paused",
        );
    }

    #[test]
    fn resolve_input_with_empty_commits_resumes_the_test() {
        // Pause → resume with `CommitCards { indices: [] }` →
        // ChaosTokenRevealed and the rest of resolution fire on the
        // second apply.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let paused = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );

        let resumed = apply(
            paused.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::CommitCards { indices: vec![] },
            }),
        );

        assert_eq!(resumed.outcome, EngineOutcome::Done);
        assert_event!(resumed.events, Event::ChaosTokenRevealed { .. });
        assert_event!(
            resumed.events,
            Event::SkillTestSucceeded { investigator, .. } if *investigator == id
        );
        assert_event!(
            resumed.events,
            Event::SkillTestEnded { investigator } if *investigator == id
        );
        assert!(
            resumed.state.in_flight_skill_test.is_none(),
            "in_flight_skill_test must clear after resolution",
        );
    }

    #[test]
    fn skill_test_pushes_and_pops_a_continuation_frame() {
        // Axis-B T4: the commit window is a `Continuation::SkillTest` frame
        // on the one stack, pushed when the test parks and popped when it
        // fully resolves.
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let paused = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );
        assert!(matches!(
            paused.outcome,
            EngineOutcome::AwaitingInput { .. }
        ));
        assert_eq!(
            paused.state.continuations,
            vec![crate::state::Continuation::SkillTest],
            "parking at the commit window pushes exactly one SkillTest frame",
        );

        let resumed = apply(
            paused.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::CommitCards { indices: vec![] },
            }),
        );
        assert_eq!(resumed.outcome, EngineOutcome::Done);
        assert!(
            resumed.state.continuations.is_empty(),
            "resolving the test pops the SkillTest frame",
        );
    }

    #[test]
    fn commit_window_discards_committed_cards_into_discard_pile() {
        // Two cards in hand; commit both. After resolution, both are
        // in the discard pile, neither in hand, and CardDiscarded
        // events fired with `from: Hand`. Icon counting is exercised
        // separately via the cards integration test (this one
        // doesn't install a registry, so icon contribution is 0).
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![CardCode::new("A"), CardCode::new("B")];
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_chaos_bag(bag_only_zero())
            .build();

        let result = apply_no_commits_with_response(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
            InputResponse::CommitCards {
                indices: vec![0, 1],
            },
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        let inv_after = &result.state.investigators[&id];
        assert!(
            inv_after.hand.is_empty(),
            "hand must be empty after commit + discard"
        );
        assert_eq!(
            inv_after.discard,
            vec![CardCode::new("B"), CardCode::new("A")],
            "committed cards land in discard (descending-index removal order)",
        );
        assert_event_count!(result.events, 2, Event::CardDiscarded { .. });
        assert_event!(
            result.events,
            Event::CardDiscarded { investigator, code, from: Zone::Hand }
                if *investigator == id && *code == CardCode::new("A")
        );
        assert_event!(
            result.events,
            Event::CardDiscarded { investigator, code, from: Zone::Hand }
                if *investigator == id && *code == CardCode::new("B")
        );
    }

    /// Helper: drive a skill-test-initiating action through with the
    /// given `InputResponse`. Used by commit-window tests that don't
    /// fit `apply_no_commits` (which always submits an empty commit).
    fn apply_no_commits_with_response(
        state: GameState,
        action: Action,
        response: InputResponse,
    ) -> crate::engine::ApplyResult {
        use crate::test_support::ScriptedResolver;
        let mut resolver = ScriptedResolver::new();
        resolver.push(response);
        crate::test_support::drive(state, action, resolver)
    }

    #[test]
    fn commit_window_rejects_out_of_bounds_index() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![CardCode::new("A")];
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_chaos_bag(bag_only_zero())
            .build();
        let paused = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );
        let bad = apply(
            paused.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::CommitCards { indices: vec![5] },
            }),
        );
        match bad.outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("out of bounds"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        // State stays paused (engine still in-flight) so a client
        // can submit a fixed-up response without re-initiating.
        assert!(bad.state.in_flight_skill_test.is_some());
    }

    #[test]
    fn commit_window_rejects_duplicate_indices() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.hand = vec![CardCode::new("A"), CardCode::new("B")];
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_chaos_bag(bag_only_zero())
            .build();
        let paused = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );
        let bad = apply(
            paused.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::CommitCards {
                    indices: vec![0, 0],
                },
            }),
        );
        match bad.outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(reason.contains("duplicate"), "unexpected reason: {reason}");
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        // State stays paused so the client can submit a fixed-up
        // response without re-initiating the test.
        assert!(bad.state.in_flight_skill_test.is_some());
    }

    #[test]
    fn non_resolve_input_action_rejects_while_skill_test_paused() {
        // While a test is paused at its commit window, the engine
        // rejects every other player action (mirrors the
        // mulligan_pending guard).
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_investigator(test_investigator(1))
            .with_active_investigator(id)
            .with_chaos_bag(bag_only_zero())
            .build();
        let paused = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );
        let rejected = apply(paused.state, Action::Player(PlayerAction::EndTurn));
        match rejected.outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("commit window"),
                    "unexpected reason: {reason}",
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        // The pause must survive the rejected action.
        assert!(rejected.state.in_flight_skill_test.is_some());
    }

    #[test]
    fn resolve_input_with_wrong_response_variant_rejects() {
        let id = InvestigatorId(1);
        let state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let paused = apply(
            state,
            Action::Player(PlayerAction::PerformSkillTest {
                investigator: id,
                skill: SkillKind::Intellect,
                difficulty: 3,
            }),
        );
        let bad = apply(
            paused.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }),
        );
        assert!(matches!(bad.outcome, EngineOutcome::Rejected { .. }));
        // Test still paused.
        assert!(bad.state.in_flight_skill_test.is_some());
    }

    #[test]
    fn resolve_input_without_any_outstanding_prompt_rejects() {
        // No prior `apply` opened a commit window — the engine has
        // nothing to resume.
        let state = GameStateBuilder::new().build();
        let result = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::CommitCards { indices: vec![] },
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn investigate_canonical_event_sequence_pins_followup_before_test_ended() {
        // Pins the post-#63 ordering: on a successful Investigate the
        // discover-clue events fire *before* SkillTestEnded, matching
        // the `SkillTestEnded` event-doc text that cleanup precedes
        // the end marker. The pre-#63 ordering ran the follow-up
        // after the bracketing end event — silently flipping that
        // back would break downstream listeners that key off the end
        // marker as "all sub-effects already applied."
        let (inv_id, _loc_id, state) = investigate_scenario(2, 2);
        let result = apply_no_commits(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        crate::assert_event_sequence!(
            result.events,
            Event::SkillTestStarted { .. },
            Event::ChaosTokenRevealed { .. },
            Event::SkillTestSucceeded { .. },
            Event::CluePlaced { .. },
            Event::LocationCluesChanged { .. },
            Event::SkillTestEnded { .. },
        );
    }

    use crate::scenario::{Resolution, ScenarioId, ScenarioModule, ScenarioRegistry};
    use crate::state::Act;

    /// `apply_resolution` that records it ran by stamping the acting
    /// investigator's resources to a sentinel value, so tests can assert
    /// the module hook (not just the event) fired.
    fn stamp_apply(
        _res: &Resolution,
        state: &mut crate::state::GameState,
        _events: &mut Vec<Event>,
    ) {
        if let Some(inv) = state.investigators.values_mut().next() {
            inv.resources = 99;
        }
    }

    fn unused_setup() -> crate::state::GameState {
        GameStateBuilder::new().build()
    }

    static STAMP_MODULE: ScenarioModule = ScenarioModule {
        resolve_symbol: None,
        setup: unused_setup,
        apply_resolution: stamp_apply,
    };

    fn stamp_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
        if id.as_str() == "stamp" {
            Some(&STAMP_MODULE)
        } else {
            None
        }
    }

    /// Build an Investigation-phase state whose current (only) act is
    /// terminal and whose investigator holds exactly enough clues to
    /// advance it — so a single `AdvanceAct` latches `Won`.
    fn terminal_act_state(scenario_id: Option<&str>) -> crate::state::GameState {
        let inv = InvestigatorId(1);
        let mut investigator = test_investigator(1);
        investigator.clues = 1;
        let mut builder = GameStateBuilder::new()
            .with_phase(crate::state::Phase::Investigation)
            .with_investigator(investigator)
            .with_active_investigator(inv)
            .with_turn_order([inv]);
        if let Some(id) = scenario_id {
            builder = builder.with_scenario_id(ScenarioId::new(id));
        }
        let mut state = builder.build();
        state.act_deck = vec![Act {
            code: CardCode("_test_act".into()),
            clue_threshold: 1,
            resolution: Some(Resolution::Won { id: "test".into() }),
            round_end_advance: None,
        }];
        state
    }

    #[test]
    fn resolution_fires_and_applies_when_latch_set_with_module() {
        let state = terminal_act_state(Some("stamp"));
        let reg = ScenarioRegistry {
            module_for: stamp_module_for,
        };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct {
                investigator: InvestigatorId(1),
            }),
            Some(&reg),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "test"
        );
        assert_eq!(
            result.state.investigators[&InvestigatorId(1)].resources,
            99,
            "apply_resolution ran"
        );
    }

    #[test]
    fn resolution_event_fires_without_a_registered_module() {
        // No registry: the event still fires (resolution is engine state),
        // but apply_resolution can't run.
        let state = terminal_act_state(Some("unknown"));
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct {
                investigator: InvestigatorId(1),
            }),
            None,
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn game_end_forced_point_is_noop_without_matching_cards() {
        // The GameEnd forced point (C5a #236) fires at resolution but is a
        // no-op with no controlled cards carrying a GameEnd ability: the
        // resolution still fires, and no TraumaSuffered is emitted.
        let state = terminal_act_state(Some("unknown"));
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct {
                investigator: InvestigatorId(1),
            }),
            None,
        );
        assert_event!(result.events, Event::ScenarioResolved { .. });
        assert_no_event!(result.events, Event::TraumaSuffered { .. });
    }

    #[test]
    fn resolution_does_not_refire_on_a_later_apply() {
        let state = terminal_act_state(Some("stamp"));
        let reg = ScenarioRegistry {
            module_for: stamp_module_for,
        };
        let first = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct {
                investigator: InvestigatorId(1),
            }),
            Some(&reg),
        );
        assert_event!(first.events, Event::ScenarioResolved { .. });
        let second = super::apply_with_scenario_registry(
            first.state,
            Action::Player(PlayerAction::EndTurn),
            Some(&reg),
        );
        assert_eq!(second.outcome, EngineOutcome::Done);
        assert_no_event!(second.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn resolution_skipped_on_rejected_outcome() {
        let inv = InvestigatorId(1);
        let mut state = terminal_act_state(Some("stamp"));
        state.investigators.get_mut(&inv).unwrap().clues = 0;
        let reg = ScenarioRegistry {
            module_for: stamp_module_for,
        };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
            Some(&reg),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_no_event!(result.events, Event::ScenarioResolved { .. });
    }

    #[test]
    fn resolution_places_no_victory_without_qualifying_locations() {
        // No victory-bearing locations in play → nothing placed, no event,
        // no panic (covers the registry-absent / no-location path).
        let state = terminal_act_state(Some("stamp"));
        let reg = ScenarioRegistry {
            module_for: stamp_module_for,
        };
        let result = super::apply_with_scenario_registry(
            state,
            Action::Player(PlayerAction::AdvanceAct {
                investigator: InvestigatorId(1),
            }),
            Some(&reg),
        );
        assert!(result.state.victory_display.is_empty());
        assert_no_event!(result.events, Event::EnteredVictoryDisplay { .. });
    }
}
