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
    use crate::event::Event;
    use crate::state::{ChaosToken, InvestigatorId, Phase, TokenModifiers, TokenResolution};
    use crate::test_support::{test_investigator, TestGame};
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

    #[test]
    fn chaos_token_drawn_with_empty_bag_is_rejected() {
        let state = TestGame::new().build();
        let result = apply(
            state,
            Action::Engine(EngineRecord::ChaosTokenDrawn {
                token: ChaosToken::Skull,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    fn bag_with_three_tokens() -> crate::state::ChaosBag {
        crate::state::ChaosBag::new([
            ChaosToken::Skull,
            ChaosToken::Numeric(1),
            ChaosToken::Numeric(-2),
        ])
    }

    /// Drive the RNG forward to figure out which token *will* be drawn
    /// next. Used by tests to construct a `ChaosTokenDrawn` action with
    /// a token that matches the RNG's expectation.
    fn peek_next_token(state: &crate::state::GameState) -> ChaosToken {
        let mut probe = state.rng.clone();
        let idx = probe.next_index(state.chaos_bag.tokens.len());
        state.chaos_bag.tokens[idx]
    }

    #[test]
    fn chaos_token_drawn_with_matching_token_succeeds_and_advances_rng() {
        let state = TestGame::new()
            .with_chaos_bag(bag_with_three_tokens())
            .with_rng_seed(42)
            .build();
        let token = peek_next_token(&state);
        let draws_before = state.rng.draws;

        let result = apply(
            state,
            Action::Engine(EngineRecord::ChaosTokenDrawn { token }),
        );

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(
            result.events,
            Event::ChaosTokenRevealed { token: t, .. } if *t == token
        );
        assert_eq!(result.state.rng.draws, draws_before + 1);
    }

    /// All seven token kinds, so a draw across this bag exercises every
    /// resolution branch.
    fn bag_with_all_token_kinds() -> crate::state::ChaosBag {
        crate::state::ChaosBag::new([
            ChaosToken::Numeric(1),
            ChaosToken::Numeric(-2),
            ChaosToken::Skull,
            ChaosToken::Cultist,
            ChaosToken::Tablet,
            ChaosToken::ElderThing,
            ChaosToken::AutoFail,
            ChaosToken::ElderSign,
        ])
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

    #[test]
    fn chaos_token_drawn_emits_resolution_for_each_token_kind() {
        // Drive the engine across all seven token kinds and assert each
        // one's emitted resolution matches the scenario modifiers and
        // the AutoFail/ElderSign special variants.
        let mut state = TestGame::new()
            .with_chaos_bag(bag_with_all_token_kinds())
            .with_token_modifiers(night_of_the_zealot_standard())
            .with_rng_seed(7)
            .build();

        let mut seen: std::collections::HashMap<ChaosToken, TokenResolution> =
            std::collections::HashMap::new();
        // Eight tokens in the bag; loop until we've witnessed each kind.
        // RNG draws with replacement (see ChaosBag::tokens semantics:
        // order doesn't matter, draw via RNG), so this terminates
        // quickly without us pre-computing the sequence.
        let kinds_to_cover = [
            ChaosToken::Numeric(1),
            ChaosToken::Numeric(-2),
            ChaosToken::Skull,
            ChaosToken::Cultist,
            ChaosToken::Tablet,
            ChaosToken::ElderThing,
            ChaosToken::AutoFail,
            ChaosToken::ElderSign,
        ];
        let mut iters = 0;
        while seen.len() < kinds_to_cover.len() {
            iters += 1;
            assert!(iters < 200, "RNG never produced full token coverage");
            let token = peek_next_token(&state);
            let result = apply(
                state,
                Action::Engine(EngineRecord::ChaosTokenDrawn { token }),
            );
            assert_eq!(result.outcome, EngineOutcome::Done);
            let resolution = result
                .events
                .iter()
                .find_map(|e| match e {
                    Event::ChaosTokenRevealed { resolution, .. } => Some(*resolution),
                    _ => None,
                })
                .expect("ChaosTokenRevealed event missing");
            seen.entry(token).or_insert(resolution);
            state = result.state;
        }

        let mods = night_of_the_zealot_standard();
        assert_eq!(seen[&ChaosToken::Numeric(1)], TokenResolution::Modifier(1));
        assert_eq!(
            seen[&ChaosToken::Numeric(-2)],
            TokenResolution::Modifier(-2),
        );
        assert_eq!(
            seen[&ChaosToken::Skull],
            TokenResolution::Modifier(mods.skull),
        );
        assert_eq!(
            seen[&ChaosToken::Cultist],
            TokenResolution::Modifier(mods.cultist),
        );
        assert_eq!(
            seen[&ChaosToken::Tablet],
            TokenResolution::Modifier(mods.tablet),
        );
        assert_eq!(
            seen[&ChaosToken::ElderThing],
            TokenResolution::Modifier(mods.elder_thing),
        );
        assert_eq!(seen[&ChaosToken::AutoFail], TokenResolution::AutoFail);
        assert_eq!(seen[&ChaosToken::ElderSign], TokenResolution::ElderSign);
    }

    #[test]
    fn chaos_token_drawn_with_wrong_token_is_rejected_and_does_not_advance_rng() {
        let state = TestGame::new()
            .with_chaos_bag(bag_with_three_tokens())
            .with_rng_seed(42)
            .build();
        let correct = peek_next_token(&state);
        // Pick any token from the bag that ISN'T the correct one.
        let wrong = state
            .chaos_bag
            .tokens
            .iter()
            .copied()
            .find(|t| *t != correct)
            .expect("bag contains at least two distinct tokens");
        let rng_before = state.rng.clone();

        let result = apply(
            state,
            Action::Engine(EngineRecord::ChaosTokenDrawn { token: wrong }),
        );

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
        assert_eq!(result.state.rng, rng_before);
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
    fn chaos_token_drawn_log_round_trips() {
        // Build a five-draw log against an initial state, then replay
        // the log from scratch and assert the resulting RNG state
        // matches. This is the core determinism property.
        let initial = TestGame::new()
            .with_chaos_bag(bag_with_three_tokens())
            .with_rng_seed(123)
            .build();

        let mut state = initial.clone();
        let mut log = Vec::new();
        for _ in 0..5 {
            let token = peek_next_token(&state);
            let action = Action::Engine(EngineRecord::ChaosTokenDrawn { token });
            log.push(action.clone());
            let result = apply(state, action);
            assert_eq!(result.outcome, EngineOutcome::Done);
            state = result.state;
        }
        let after_first_pass = state;

        // Replay against the original initial state.
        let mut replay = initial;
        for action in log {
            let result = apply(replay, action);
            assert_eq!(result.outcome, EngineOutcome::Done);
            replay = result.state;
        }

        assert_eq!(after_first_pass.rng, replay.rng);
        assert_eq!(after_first_pass.rng.draws, 5);
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
}
