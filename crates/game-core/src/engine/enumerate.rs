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
    _state: &GameState,
    _investigator: InvestigatorId,
    out: &mut Vec<PlayerAction>,
) {
    out.push(PlayerAction::EndTurn);
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
}
