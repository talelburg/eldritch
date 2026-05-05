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
    use std::collections::BTreeMap;

    use crate::action::{Action, EngineRecord, InputResponse, PlayerAction};
    use crate::event::Event;
    use crate::state::{ChaosBag, GameState, Investigator, InvestigatorId, Phase, Skills};

    use super::{apply, EngineOutcome};

    fn empty_state() -> GameState {
        GameState {
            investigators: BTreeMap::new(),
            locations: BTreeMap::new(),
            chaos_bag: ChaosBag::new([]),
            phase: Phase::Mythos,
            round: 0,
            active_investigator: None,
            turn_order: Vec::new(),
        }
    }

    fn investigator(id: u32, actions_remaining: u8) -> Investigator {
        Investigator {
            id: InvestigatorId(id),
            name: format!("Test Investigator {id}"),
            current_location: None,
            skills: Skills {
                willpower: 3,
                intellect: 3,
                combat: 3,
                agility: 3,
            },
            max_health: 8,
            damage: 0,
            max_sanity: 8,
            horror: 0,
            clues: 0,
            resources: 5,
            actions_remaining,
        }
    }

    #[test]
    fn start_scenario_emits_scenario_started_and_sets_round_to_one() {
        let state = empty_state();
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(result.events, vec![Event::ScenarioStarted]);
        assert_eq!(result.state.round, 1);
    }

    #[test]
    fn start_scenario_on_already_started_state_is_rejected() {
        let mut state = empty_state();
        state.round = 7;
        let result = apply(state, Action::Player(PlayerAction::StartScenario));

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.state.round, 7);
        assert!(result.events.is_empty());
    }

    fn investigation_state_with_active(actions_remaining: u8) -> (GameState, InvestigatorId) {
        let mut state = empty_state();
        let id = InvestigatorId(1);
        state
            .investigators
            .insert(id, investigator(1, actions_remaining));
        state.active_investigator = Some(id);
        state.phase = Phase::Investigation;
        (state, id)
    }

    #[test]
    fn end_turn_drains_actions_and_emits_turn_ended() {
        let (state, id) = investigation_state_with_active(3);

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert_eq!(result.outcome, EngineOutcome::Done);
        assert_eq!(
            result.events,
            vec![
                Event::ActionsRemainingChanged {
                    investigator: id,
                    new_count: 0
                },
                Event::TurnEnded { investigator: id },
            ]
        );
        assert_eq!(result.state.investigators[&id].actions_remaining, 0);
    }

    #[test]
    fn end_turn_with_no_active_investigator_is_rejected() {
        let mut state = empty_state();
        state.phase = Phase::Investigation;

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn end_turn_outside_investigation_phase_is_rejected() {
        let mut state = empty_state();
        let id = InvestigatorId(1);
        state.investigators.insert(id, investigator(1, 3));
        state.active_investigator = Some(id);
        state.phase = Phase::Mythos;

        let result = apply(state, Action::Player(PlayerAction::EndTurn));

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn rejected_actions_do_not_mutate_state() {
        let state = empty_state();
        let before = state.clone();
        // ResolveInput is a Phase-1 stub — guaranteed to be Rejected.
        let result = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            }),
        );

        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert_eq!(result.events, Vec::<Event>::new());
        // State equality requires a manual field check since GameState
        // does not derive PartialEq (deliberately — it may be expensive).
        assert_eq!(result.state.round, before.round);
        assert_eq!(result.state.phase, before.phase);
        assert_eq!(result.state.active_investigator, before.active_investigator);
    }

    #[test]
    fn chaos_token_drawn_engine_record_is_rejected_phase_one() {
        let state = empty_state();
        let result = apply(
            state,
            Action::Engine(EngineRecord::ChaosTokenDrawn {
                token: crate::state::ChaosToken::Skull,
            }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }

    #[test]
    fn deck_shuffled_engine_record_is_rejected_phase_one() {
        let state = empty_state();
        let result = apply(
            state,
            Action::Engine(EngineRecord::DeckShuffled { seed: 42 }),
        );
        assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
        assert!(result.events.is_empty());
    }
}
