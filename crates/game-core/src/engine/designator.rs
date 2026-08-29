//! Whether an investigator can perform the action a bold
//! [`ActionDesignator`] names.
//!
//! `glossary/Ability.md`, "Action Designators", verbatim:
//!
//! > Some abilities have bold action designators (such as **Fight**, **Evade**,
//! > **Investigate**, or **Move**). Activating such an ability performs the
//! > designated action as described in the rules, but modified in the manner
//! > described by the ability.
//!
//! Since the designator *performs* (#805), "can this ability initiate" and "can
//! this basic action be taken" are the same question asked of the same
//! designator, and [`can_perform`] is the one place that answers it. Three
//! callers ask: the basic-action handlers, the activation validator
//! (`check_activate_ability`, pre-cost), and the turn-menu enumerator — which
//! filters through the validator, so menu and handler cannot disagree. A check
//! only one of the paths applies is precisely the bug shape #754 was for the
//! action surcharge.

use std::borrow::Cow;

use crate::dsl::ActionDesignator;
use crate::state::{EnemyId, GameState, InvestigatorId, LocationId};

/// Whether `investigator` can perform the action `designator` names, ignoring
/// which of several legal targets will end up chosen.
///
/// Read **before any cost is paid**, so a Flashlight cannot spend its supply
/// with nothing to investigate and a Knife cannot discard itself for no legal
/// target. What it does *not* do is pick the target: on 2+ candidates the
/// evaluator suspends for a `PickSingle`, and only the empty case is a
/// rejection here.
///
/// - **Fight** — needs ≥1 enemy *at your location*. Scope is co-located, not
///   engaged-only: per RR you choose an enemy at your location to attack and
///   need not already be engaged (an Aloof enemy, or one engaged with another
///   investigator in MP, is a legal target). #451.
/// - **Investigate** — needs the controller at a *revealed* location to test.
/// - **Parley** — performs nothing (`glossary/Parley.md` gives it no
///   procedure), so nothing can be missing. Whether the ability is worth
///   initiating is then its residual effect's question, which the RR initiation
///   gate asks separately.
/// - **Resign** — eliminating the controller is always available to an
///   investigator who reached the ability at all.
/// - **Evade** / **Move** — rejected. No implemented card prints either, so
///   neither variant carries the modification a real one would, and the engine
///   says so rather than performing a guess at the printed text. `TODO(#805)`:
///   the shape lands with the first card that prints one.
pub(crate) fn can_perform(
    state: &GameState,
    investigator: InvestigatorId,
    designator: &ActionDesignator,
) -> Result<(), Cow<'static, str>> {
    match designator {
        ActionDesignator::Fight { .. } => {
            if fight_candidates(state, investigator).is_empty() {
                return Err(
                    "a Fight ability needs an enemy at your location (none co-located)".into(),
                );
            }
            Ok(())
        }
        ActionDesignator::Investigate { .. } => {
            if investigate_location(state, investigator).is_none() {
                return Err(
                    "an Investigate ability needs a revealed location to investigate".into(),
                );
            }
            Ok(())
        }
        ActionDesignator::Parley | ActionDesignator::Resign => Ok(()),
        ActionDesignator::Evade | ActionDesignator::Move => Err(format!(
            "a designated {designator:?} is not implemented: no corpus card prints one, so the \
             modification it would carry has no shape yet",
        )
        .into()),
    }
}

/// The enemies a **Fight** may target: every enemy at `investigator`'s
/// location, in ascending [`EnemyId`] order.
///
/// The same list the evaluator's target grounding offers, so the pre-cost gate
/// and the pick cannot disagree about what counts as a candidate.
pub(crate) fn fight_candidates(state: &GameState, investigator: InvestigatorId) -> Vec<EnemyId> {
    crate::engine::dispatch::combat::enemies_in_scope(
        state,
        investigator,
        crate::engine::dispatch::combat::fight_target_scope(),
    )
}

/// The location an **Investigate** would test: `investigator`'s current
/// location, if it exists and is revealed. `None` is the lapsed/ineligible
/// case, which reads as a rejection pre-cost and as a suppression on resume.
pub(crate) fn investigate_location(
    state: &GameState,
    investigator: InvestigatorId,
) -> Option<LocationId> {
    state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location)
        .filter(|id| state.locations.get(id).is_some_and(|loc| loc.revealed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::IntExpr;
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};

    const ME: InvestigatorId = InvestigatorId(1);
    const HERE: LocationId = LocationId(1);

    fn fight() -> ActionDesignator {
        ActionDesignator::Fight {
            combat_modifier: IntExpr::Lit(1),
            extra_damage: IntExpr::Lit(0),
        }
    }

    fn investigate() -> ActionDesignator {
        ActionDesignator::Investigate {
            shroud_modifier: IntExpr::Lit(-2),
        }
    }

    /// A board with the investigator at a revealed [`HERE`] and `enemies`
    /// co-located there.
    fn board(revealed: bool, enemies: u32) -> GameState {
        let mut inv = test_investigator(1);
        inv.current_location = Some(HERE);
        let mut loc = test_location(1, "Study");
        loc.revealed = revealed;
        let mut builder = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc);
        for n in 0..enemies {
            let mut enemy = test_enemy(n + 1, "Ghoul");
            enemy.current_location = Some(HERE);
            builder = builder.with_enemy(enemy);
        }
        builder.build()
    }

    #[test]
    fn a_fight_needs_a_co_located_enemy() {
        assert!(can_perform(&board(true, 0), ME, &fight()).is_err());
        assert!(can_perform(&board(true, 1), ME, &fight()).is_ok());
        // 2+ is the pick case, not a rejection.
        assert!(can_perform(&board(true, 2), ME, &fight()).is_ok());
    }

    #[test]
    fn an_investigate_needs_a_revealed_location() {
        assert!(can_perform(&board(false, 0), ME, &investigate()).is_err());
        assert!(can_perform(&board(true, 0), ME, &investigate()).is_ok());
    }

    /// Parley performs nothing and Resign always can, on the emptiest board
    /// that still has the investigator on it.
    #[test]
    fn parley_and_resign_need_nothing_of_the_board() {
        let state = board(false, 0);
        assert!(can_perform(&state, ME, &ActionDesignator::Parley).is_ok());
        assert!(can_perform(&state, ME, &ActionDesignator::Resign).is_ok());
    }

    /// The two designators no implemented card prints reject rather than
    /// performing a guess — and they reject *pre-cost*, so nothing is spent.
    #[test]
    fn an_unprinted_designator_rejects_rather_than_guessing() {
        let state = board(true, 1);
        for d in [ActionDesignator::Evade, ActionDesignator::Move] {
            assert!(can_perform(&state, ME, &d).is_err(), "{d:?}");
        }
    }
}
