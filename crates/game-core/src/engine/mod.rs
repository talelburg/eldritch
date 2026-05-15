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
    use crate::state::EnemyId;
    use crate::state::{
        CardCode, ChaosToken, DefeatCause, GameState, InvestigatorId, Phase, SkillKind, Status,
        TokenModifiers, TokenResolution,
    };
    use crate::test_support::{test_enemy, test_investigator, test_location, TestGame};
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

        // Mulligan past the setup window.
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
        assert_eq!(result.state.round, 2);
        assert_eq!(result.state.phase, Phase::Investigation);
        assert_eq!(result.state.active_investigator, Some(id));
        assert_eq!(result.state.investigators[&id].actions_remaining, 3);
        assert_event_count!(result.events, 4, Event::PhaseEnded { .. });
        assert_event_count!(result.events, 4, Event::PhaseStarted { .. });
    }

    #[test]
    fn deck_shuffled_engine_record_with_unknown_investigator_is_rejected() {
        let state = TestGame::new().build();
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
        let state = TestGame::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .with_rng_seed(42)
            .build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

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
        let state = TestGame::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([id])
            .build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

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
        let state = TestGame::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .with_rng_seed(7)
            .build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

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
        let state_a = TestGame::new()
            .with_investigator(inv.clone())
            .with_turn_order([id])
            .with_rng_seed(123)
            .build();
        let state_b = TestGame::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .with_rng_seed(123)
            .build();

        let result_a = apply(state_a, Action::Player(PlayerAction::StartScenario));
        let result_b = apply(state_b, Action::Player(PlayerAction::StartScenario));

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
        let state = TestGame::new()
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
        let mut tg = TestGame::new().with_rng_seed(2026);
        for id in ids {
            let mut inv = test_investigator(id.0);
            inv.deck = make_test_deck(8);
            tg = tg.with_investigator(inv);
        }
        let result = apply(tg.build(), Action::Player(PlayerAction::StartScenario));
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
        let state = TestGame::new()
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
        let state = TestGame::new()
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
        let result = apply(
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
        let result = apply(
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
    fn fight_defeats_enemy_when_damage_reaches_max_health() {
        // Enemy at 1/2 already; Fight success → damage 2, defeated,
        // removed from state, engagement cleared.
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.enemies.get_mut(&enemy_id).unwrap().damage = 1;
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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

        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        let result = apply(
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
        // Move suppressed: investigator and engaged enemy stay at the
        // origin; no InvestigatorMoved event.
        assert_no_event!(result.events, Event::InvestigatorMoved { .. });
        assert_eq!(
            result.state.investigators[&inv_id].current_location,
            Some(a)
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
        let result = apply(
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
        // the second is a no-op (take_damage skips defeated).
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
        let state = TestGame::new()
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
    fn defeated_investigator_cannot_fight() {
        let (inv_id, enemy_id, mut state) = fight_evade_scenario();
        state.investigators.get_mut(&inv_id).unwrap().status = Status::Killed;
        let result = apply(
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
        let result = apply(
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
        let state = TestGame::new()
            .with_investigator(inv)
            .with_chaos_bag(bag_only_zero())
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
        let state = TestGame::new()
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
    /// of 5 cards + a remaining deck of 5, mulligan window open.
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
        let state = TestGame::new()
            .with_investigator(inv)
            .with_rng_seed(2026)
            .with_mulligan_window_open()
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
        assert!(inv.mulligan_used);
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
        // One-shot consumed.
        assert!(inv.mulligan_used);
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
    fn mulligan_after_window_closed_is_rejected() {
        let (id, mut state) = mulligan_scenario();
        state.mulligan_window = false;
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
        let (id, mut state) = mulligan_scenario();
        state.investigators.get_mut(&id).unwrap().status = Status::Killed;
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
    fn start_scenario_opens_mulligan_window() {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.deck = make_test_deck(10);
        let state = TestGame::new()
            .with_investigator(inv)
            .with_turn_order([id])
            .build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert!(result.state.mulligan_window);
    }

    #[test]
    fn non_mulligan_action_during_mulligan_window_is_rejected() {
        // Non-Mulligan player actions are gated by the mulligan
        // window: the engine refuses Move/Investigate/etc. until
        // every investigator has signaled their mulligan choice.
        let id = InvestigatorId(1);
        let a = crate::state::LocationId(10);
        let b = crate::state::LocationId(11);
        let mut inv = test_investigator(1);
        inv.current_location = Some(a);
        inv.actions_remaining = 3;
        let mut loc_a = test_location(10, "A");
        loc_a.connections = vec![b];
        let state = TestGame::new()
            .with_investigator(inv)
            .with_location(loc_a)
            .with_location(test_location(11, "B"))
            .with_phase(Phase::Investigation)
            .with_active_investigator(id)
            .with_mulligan_window_open()
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
    fn solo_mulligan_closes_the_window() {
        // Single-investigator scenario: as soon as that one
        // investigator mulligans (empty redraw counts), all
        // investigators have mulliganed and the window closes.
        let (id, state) = mulligan_scenario();
        let result = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: id,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(result.outcome, EngineOutcome::Done);
        assert!(!result.state.mulligan_window);
    }

    #[test]
    fn multi_investigator_first_mulligan_keeps_window_open() {
        // Two investigators; only one mulligans. Window stays open
        // until the other one also submits Mulligan.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        let mut b = test_investigator(2);
        a.hand = vec![CardCode::new("a-0")];
        b.hand = vec![CardCode::new("b-0")];
        let state = TestGame::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_mulligan_window_open()
            .build();

        let after_first = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(after_first.outcome, EngineOutcome::Done);
        assert!(after_first.state.mulligan_window);
        assert!(after_first.state.investigators[&inv1].mulligan_used);
        assert!(!after_first.state.investigators[&inv2].mulligan_used);

        let after_second = apply(
            after_first.state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        );
        assert_eq!(after_second.outcome, EngineOutcome::Done);
        assert!(!after_second.state.mulligan_window);
    }

    #[test]
    fn multi_investigator_mulligan_order_does_not_matter() {
        // Same shape as the previous test but with the second
        // investigator mulliganing first — exercises that the
        // window-closure predicate is order-independent.
        let inv1 = InvestigatorId(1);
        let inv2 = InvestigatorId(2);
        let mut a = test_investigator(1);
        let mut b = test_investigator(2);
        a.hand = vec![CardCode::new("a-0")];
        b.hand = vec![CardCode::new("b-0")];
        let state = TestGame::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_mulligan_window_open()
            .build();

        let after_inv2 = apply(
            state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv2,
                indices_to_redraw: vec![],
            }),
        );
        assert!(after_inv2.state.mulligan_window);
        let after_inv1 = apply(
            after_inv2.state,
            Action::Player(PlayerAction::Mulligan {
                investigator: inv1,
                indices_to_redraw: vec![],
            }),
        );
        assert!(!after_inv1.state.mulligan_window);
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
        let state = TestGame::new()
            .with_investigator(a)
            .with_investigator(b)
            .with_mulligan_window_open()
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
        assert!(after_inv1.state.mulligan_window); // inv2 hasn't yet
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
        assert!(!after_inv2.state.mulligan_window);
        assert_event!(
            after_inv2.events,
            Event::MulliganPerformed { investigator, redrawn_count: 0 }
                if *investigator == inv2
        );
        // inv2's zones untouched by their no-op mulligan.
        let inv2_after = &after_inv2.state.investigators[&inv2];
        assert_eq!(inv2_after.hand, original_inv2_hand);
        assert_eq!(inv2_after.deck, original_inv2_deck);
        assert!(inv2_after.mulligan_used);
    }
}
