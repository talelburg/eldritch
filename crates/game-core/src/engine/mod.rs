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

mod dispatch;
pub mod evaluator;
mod outcome;

pub use evaluator::{apply_effect, EvalContext};
pub use outcome::{EngineOutcome, InputRequest, ResumeToken};

use crate::action::Action;
use crate::event::Event;
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
/// must be unchanged from the input. `apply` enforces this for the
/// event list (it clears events post-dispatch on rejection) but **not**
/// for state — handlers are expected to validate before mutating.
/// TODO(#17+): once non-trivial handlers exist, refactor to a strict
/// validate-first / apply-second two-phase shape so this is structural
/// rather than a per-handler convention.
pub fn apply(state: GameState, action: Action) -> ApplyResult {
    let mut state = state;
    let mut events = Vec::new();
    let outcome = match action {
        Action::Player(p) => dispatch::apply_player_action(&mut state, &mut events, &p),
        Action::Engine(e) => dispatch::apply_engine_record(&mut state, &mut events, &e),
    };
    if matches!(outcome, EngineOutcome::Rejected { .. }) {
        // Belt-and-suspenders: handlers are expected to validate before
        // mutating, so events should already be empty here. Clear
        // anyway in case a handler accidentally pushed before bailing.
        events.clear();
    }
    ApplyResult {
        state,
        events,
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use crate::action::{Action, EngineRecord, InputResponse, PlayerAction};
    use crate::event::{Event, FailureReason};
    use crate::state::{
        ChaosToken, GameState, InvestigatorId, Phase, SkillKind, TokenModifiers, TokenResolution,
    };
    use crate::test_support::{test_investigator, test_location, TestGame};
    use crate::{assert_event, assert_event_count, assert_no_event};

    use super::{apply, EngineOutcome};

    #[test]
    fn start_scenario_advances_to_investigation_with_round_one() {
        let id = InvestigatorId(1);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.round, 1);
        assert_eq!(result.state.phase, Phase::Investigation);
        assert_eq!(result.state.active_investigator, Some(id));
        assert_eq!(result.state.investigators[&id].actions_remaining, 3);

        assert_event!(result.events, Event::ScenarioStarted);
        assert_event!(
            result.events,
            Event::PhaseStarted {
                phase: Phase::Mythos
            }
        );
        assert_event!(
            result.events,
            Event::PhaseEnded {
                phase: Phase::Mythos
            }
        );
        assert_event!(
            result.events,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        );
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 3 } if *investigator == id
        );
    }

    #[test]
    fn start_scenario_on_already_started_state_is_rejected() {
        let state = TestGame::new().with_round(7).build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.round, 7);
        assert!(result.events.is_empty());
    }

    #[test]
    fn end_turn_drains_actions_and_emits_turn_ended() {
        let id = InvestigatorId(1);
        let mut roland = test_investigator(1);
        roland.actions_remaining = 3;
        let state = TestGame::new()
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
        let state = TestGame::new().with_phase(Phase::Investigation).build();

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn end_turn_outside_investigation_phase_is_rejected() {
        let id = InvestigatorId(1);
        let state = TestGame::new()
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
        let state = TestGame::new().build();
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
        let state = TestGame::new().with_chaos_bag(bag_only_zero()).build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply(
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
        let state = TestGame::new()
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
        let state = TestGame::new()
            .with_investigator(strong)
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(bag_only_zero())
            .build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(high)
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::AutoFail]))
            .build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::AutoFail]))
            .build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::Skull]))
            .with_token_modifiers(TokenModifiers {
                skull: -6,
                ..TokenModifiers::default()
            })
            .build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::ElderSign]))
            .build();
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_chaos_bag(crate::state::ChaosBag::new([ChaosToken::Skull]))
            .with_token_modifiers(night_of_the_zealot_standard())
            .build();
        let result = apply(
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
    fn perform_skill_test_advances_rng_and_log_round_trips() {
        // Determinism: applying the same PerformSkillTest action twice
        // from identical initial state produces identical post-state.
        let id = InvestigatorId(1);
        let initial = TestGame::new()
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

        let first = apply(initial.clone(), action.clone());
        let second = apply(initial, action);

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
        let state = TestGame::new()
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
            state,
            Action::Player(PlayerAction::Investigate {
                investigator: inv_id,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn full_round_advances_through_all_phases_with_two_investigators() {
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_turn_order([inv1, inv2])
            .build();

        // StartScenario: round 0 → 1, phase Mythos → Investigation,
        // first investigator becomes active with 3 actions.
        let result = apply(state, Action::Player(PlayerAction::StartScenario));
        let state = result.state;
        assert_eq!(state.round, 1);
        assert_eq!(state.phase, Phase::Investigation);
        assert_eq!(state.active_investigator, Some(inv1));
        assert_eq!(state.investigators[&inv1].actions_remaining, 3);

        // First EndTurn: rotate to inv2 within Investigation.
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
        assert_event!(
            result.events,
            Event::ActionsRemainingChanged { investigator, new_count: 3 } if *investigator == inv2
        );
        // No phase transition yet, and EndTurn must never re-emit
        // ScenarioStarted.
        assert_no_event!(result.events, Event::PhaseEnded { .. });
        assert_no_event!(result.events, Event::PhaseStarted { .. });
        assert_no_event!(result.events, Event::ScenarioStarted);

        // Second EndTurn: tick through Enemy → Upkeep → Mythos (round
        // bumps) → Investigation, with inv1 active again at full
        // actions.
        let result = apply(state, Action::Player(PlayerAction::EndTurn));
        let state = result.state;
        assert_eq!(state.round, 2);
        assert_eq!(state.phase, Phase::Investigation);
        assert_eq!(state.active_investigator, Some(inv1));
        assert_eq!(state.investigators[&inv1].actions_remaining, 3);
        assert_eq!(state.investigators[&inv2].actions_remaining, 0);

        // All four phase-end / phase-start pairs fired during the
        // second EndTurn's auto-advance — exactly four of each, no more
        // and no less.
        assert_event_count!(result.events, 4, Event::PhaseEnded { .. });
        assert_event_count!(result.events, 4, Event::PhaseStarted { .. });
        for phase in [
            Phase::Investigation,
            Phase::Enemy,
            Phase::Upkeep,
            Phase::Mythos,
        ] {
            assert_event!(result.events, Event::PhaseEnded { phase: p } if *p == phase);
        }
        for phase in [
            Phase::Enemy,
            Phase::Upkeep,
            Phase::Mythos,
            Phase::Investigation,
        ] {
            assert_event!(result.events, Event::PhaseStarted { phase: p } if *p == phase);
        }
        // EndTurn must never re-emit ScenarioStarted.
        assert_no_event!(result.events, Event::ScenarioStarted);
    }

    #[test]
    fn solo_investigator_round_advances_on_single_end_turn() {
        // Degenerate edge: with only one investigator in turn_order,
        // their EndTurn is also the *last* EndTurn, so it must trigger
        // the full phase auto-advance plus round bump in one step.
        let id = InvestigatorId(1);
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();

        let after_start = apply(state, Action::Player(PlayerAction::StartScenario)).state;
        assert_eq!(after_start.round, 1);
        assert_eq!(after_start.active_investigator, Some(id));

        let result = apply(after_start, Action::Player(PlayerAction::EndTurn));
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.state.round, 2);
        assert_eq!(result.state.phase, Phase::Investigation);
        assert_eq!(result.state.active_investigator, Some(id));
        assert_eq!(result.state.investigators[&id].actions_remaining, 3);
        assert_event_count!(result.events, 4, Event::PhaseEnded { .. });
        assert_event_count!(result.events, 4, Event::PhaseStarted { .. });
    }

    #[test]
    fn deck_shuffled_engine_record_is_rejected_phase_one() {
        let state = TestGame::new().build();
        let result = apply(
            state,
            Action::Engine(EngineRecord::DeckShuffled { seed: 42 }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
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
}
