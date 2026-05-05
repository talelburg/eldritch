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
mod outcome;

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
    use crate::state::{ChaosToken, InvestigatorId, Phase};
    use crate::test_support::{test_investigator, TestGame};
    use crate::{assert_event, assert_event_count, assert_no_event};

    use super::{apply, EngineOutcome};

    #[test]
    fn start_scenario_emits_scenario_started_and_sets_round_to_one() {
        let state = TestGame::new().build();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_event!(result.events, Event::ScenarioStarted);
        assert_event_count!(result.events, 1, _);
        assert_eq!(result.state.round, 1);
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
            Event::ChaosTokenRevealed { token: t, modifier: 0 } if *t == token
        );
        assert_eq!(result.state.rng.draws, draws_before + 1);
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
