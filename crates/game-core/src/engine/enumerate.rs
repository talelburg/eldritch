//! The legal-action enumerator (slice 2a-ii, #393): the legal `PlayerAction`s
//! for the active investigator at the open turn. Read-only; nothing dispatches
//! through it yet (routing is 2b) — it shares the handlers' legality predicates
//! so the enumeration matches handler-acceptance by construction.

use crate::action::PlayerAction;
use crate::state::{Continuation, GameState, InvestigatorId};

/// The legal [`PlayerAction`]s the active investigator may take at the open
/// turn, in stable order (position = the future `OptionId`). Empty unless an
/// [`InvestigatorTurn`](Continuation::InvestigatorTurn) frame is on top — the
/// only point gameplay actions are taken (slice 2a-ii-1, #393).
///
/// Read-only and side-effect-free. Each action is included iff the same
/// legality predicate the handler uses accepts it, so the enumeration matches
/// handler-acceptance by construction (routing typed dispatch through it is 2b).
#[must_use]
pub fn legal_actions(state: &GameState) -> Vec<PlayerAction> {
    let Some(Continuation::InvestigatorTurn { investigator, .. }) = state.continuations.last()
    else {
        return Vec::new();
    };
    let investigator = *investigator;
    let mut actions = Vec::new();
    push_basic_actions(state, investigator, &mut actions);
    actions
}

/// Append the basic actions legal for `investigator`. `EndTurn` is always legal
/// at the open turn (the handler only needs an active investigator, guaranteed
/// here). Later tasks add Resource/Draw/Investigate/Move.
fn push_basic_actions(
    state: &GameState,
    investigator: InvestigatorId,
    out: &mut Vec<PlayerAction>,
) {
    use crate::engine::dispatch::actions::{action_cost, validate_basic_action};

    // EndTurn: always legal at the open turn (no action point required).
    out.push(PlayerAction::EndTurn);

    // Resource / Draw / Investigate share the basic-action prologue (phase +
    // active + Status::Active + actions_remaining >= 1). Investigate adds a
    // revealed-current-location gate.
    if let Ok(inv) = validate_basic_action(state, "enumerate", investigator) {
        out.push(PlayerAction::Resource { investigator });
        out.push(PlayerAction::Draw { investigator });
        if let Some(loc_id) = inv.current_location {
            if state.locations.get(&loc_id).is_some_and(|l| l.revealed) {
                out.push(PlayerAction::Investigate { investigator });
            }
        }
    }

    // Move uses its own prefix (the action-point check folds into the cost):
    // phase Investigation + active + Status::Active + a current location +
    // affordable, with one option per connected destination in state.
    let Some(inv) = state.investigators.get(&investigator) else {
        return;
    };
    if state.phase != crate::state::Phase::Investigation
        || state.active_investigator != Some(investigator)
        || inv.status != crate::state::Status::Active
    {
        return;
    }
    let Some(from) = inv.current_location else {
        return;
    };
    if action_cost(state, investigator, crate::dsl::ActionClass::Move) > inv.actions_remaining {
        return;
    }
    let Some(from_loc) = state.locations.get(&from) else {
        return;
    };
    for &dest in &from_loc.connections {
        if dest != from && state.locations.contains_key(&dest) {
            out.push(PlayerAction::Move {
                investigator,
                destination: dest,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::action::PlayerAction;
    use crate::engine::enumerate::legal_actions;
    use crate::state::{Continuation, InvestigationResume, InvestigatorId, Phase};
    use crate::test_support::{test_investigator, GameStateBuilder};

    /// Build a single-investigator open-turn state (`InvestigatorTurn` frame on
    /// top of the `InvestigationPhase` anchor), the shape `legal_actions` enumerates.
    fn open_turn_state() -> crate::state::GameState {
        GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            // A realistic board has a non-empty chaos bag — skill-test-initiating
            // actions (Investigate) reject on an empty bag (a malformed-state
            // guard the enumerator does not replicate; real bags are never empty).
            .with_chaos_bag(crate::state::ChaosBag::new([
                crate::state::ChaosToken::Numeric(0),
            ]))
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            })
            .with_investigator_turn(InvestigatorId(1))
            .build()
    }

    #[test]
    fn end_turn_is_always_offered_at_the_open_turn() {
        let state = open_turn_state();
        assert!(legal_actions(&state).contains(&PlayerAction::EndTurn));
    }

    #[test]
    fn no_actions_when_not_the_open_turn() {
        // No InvestigatorTurn frame on top (empty stack) → nothing to offer.
        let state = GameStateBuilder::default()
            .with_investigator(test_investigator(1))
            .with_phase(Phase::Investigation)
            .with_active_investigator(InvestigatorId(1))
            .build();
        assert!(legal_actions(&state).is_empty());
    }

    #[test]
    fn basic_actions_offered_with_a_revealed_location_and_an_action() {
        let mut state = open_turn_state();
        // Place the investigator on a revealed location so Investigate is legal.
        let loc = crate::test_support::test_location(10, "Study");
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state.locations.get_mut(&loc_id).unwrap().revealed = true;
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;

        let actions = legal_actions(&state);
        assert!(actions.contains(&PlayerAction::Resource {
            investigator: InvestigatorId(1)
        }));
        assert!(actions.contains(&PlayerAction::Draw {
            investigator: InvestigatorId(1)
        }));
        assert!(actions.contains(&PlayerAction::Investigate {
            investigator: InvestigatorId(1)
        }));
    }

    #[test]
    fn no_action_points_offers_only_end_turn() {
        let mut state = open_turn_state();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 0;
        // With 0 actions, only EndTurn (which needs no action point) is legal.
        assert_eq!(legal_actions(&state), vec![PlayerAction::EndTurn]);
    }

    #[test]
    fn investigate_absent_on_an_unrevealed_location() {
        let mut state = open_turn_state();
        let mut loc = crate::test_support::test_location(10, "Study");
        loc.revealed = false;
        let loc_id = loc.id;
        state.locations.insert(loc_id, loc);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(loc_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;
        assert!(!legal_actions(&state).contains(&PlayerAction::Investigate {
            investigator: InvestigatorId(1)
        }));
    }

    #[test]
    fn move_offers_one_option_per_connected_destination() {
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b_id, b);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(a_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;

        let actions = legal_actions(&state);
        assert!(actions.contains(&PlayerAction::Move {
            investigator: InvestigatorId(1),
            destination: b_id,
        }));
        // No self-move.
        assert!(!actions.contains(&PlayerAction::Move {
            investigator: InvestigatorId(1),
            destination: a_id,
        }));
    }

    #[test]
    fn move_absent_when_unaffordable() {
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b_id, b);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(a_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 0;
        assert!(!legal_actions(&state)
            .iter()
            .any(|a| matches!(a, PlayerAction::Move { .. })));
    }

    #[test]
    fn every_enumerated_action_is_accepted_by_its_handler() {
        // The cross-check that makes "defer routing" safe: each enumerated
        // action applies without Rejected (Done or AwaitingInput both mean
        // "accepted"). Apply to a fresh clone per action. The board has a
        // connected, revealed destination so a Move is enumerated and checked too.
        let mut state = open_turn_state();
        let mut a = crate::test_support::test_location(10, "A");
        let b = crate::test_support::test_location(11, "B");
        a.connections = vec![b.id];
        let (a_id, _b_id) = (a.id, b.id);
        state.locations.insert(a_id, a);
        state.locations.insert(b.id, b);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(a_id);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .actions_remaining = 3;

        for action in legal_actions(&state) {
            let result = crate::apply(state.clone(), crate::Action::Player(action.clone()));
            assert!(
                !matches!(result.outcome, crate::EngineOutcome::Rejected { .. }),
                "enumerated action {action:?} was rejected by its handler: {:?}",
                result.outcome,
            );
        }
    }
}
