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
//! Since the designator *performs* (#805), this module owns what the action
//! needs of the board, and every path that takes one reads it from here. A
//! check only one of the paths applies is precisely the bug shape #754 was for
//! the action surcharge.
//!
//! [`can_perform`] answers *"can this designated ability initiate at all"* —
//! whether **some** legal target exists, never which one. Its callers are the
//! activation validator (`check_activate_ability`) and the play validator
//! (`check_play_card`, for an event's designated `OnPlay` ability), both
//! pre-cost, which the turn-menu enumerator filters through in turn, so menu
//! and handler cannot disagree about what is offerable.
//!
//! What it answers *from* is the basic action's own `candidates`
//! (`actions::fight::candidates`, `actions::evade::candidates`,
//! `actions::investigate::candidates`), the one
//! list the basic action's handler and the turn menu read too. A basic action
//! names its target up front, so its handler asks whether *this* enemy is in the
//! list, a question `can_perform` deliberately does not ask. Sharing the list
//! rather than the predicate is what keeps a designated **Fight** and the basic
//! Fight action agreeing on what a legal target is; the evaluator's target
//! grounding reads the same list again.

use std::borrow::Cow;

use card_dsl::dsl::ActionDesignator;

use crate::engine::dispatch::actions::{self, evade, fight, investigate};
use crate::state::{GameState, InvestigatorId};

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
/// - **Evade** — needs ≥1 enemy *engaged with you*. `glossary/Evade_Action.md`:
///   *"Unlike the fight and engage action, an investigator can only perform an
///   evade action against an enemy engaged with him or her."*
/// - **Investigate** — needs the controller at a *revealed* location to test.
/// - **Parley** — performs nothing (`glossary/Parley.md` gives it no
///   procedure), so nothing can be missing. Whether the ability is worth
///   initiating is then its residual effect's question, which the RR initiation
///   gate asks separately.
/// - **Resign** — eliminating the controller is always available to an
///   investigator who reached the ability at all.
/// - **`AutomaticEvade`** — performs nothing, as Parley does: no test, and no
///   engaged enemy needed (Cunning Distraction 01078's ruling,
///   <https://arkhamdb.com/card/01078>: *"even those that you're not engaged
///   with"*). Whether there is an enemy to evade is its residual effect's
///   question.
/// - **Move** — rejected, with the reason
///   [`actions::designated_unimplemented_move`] gives (`TODO(#1059)`). The
///   variant carries no modification because no corpus card prints a bold
///   **Move**, so the engine says so rather than performing a guess.
pub(crate) fn can_perform(
    state: &GameState,
    investigator: InvestigatorId,
    designator: &ActionDesignator,
) -> Result<(), Cow<'static, str>> {
    match designator {
        ActionDesignator::Fight { .. } => {
            if fight::candidates(state, investigator).is_empty() {
                return Err(
                    "a Fight ability needs an enemy at your location (none co-located)".into(),
                );
            }
            Ok(())
        }
        ActionDesignator::Investigate { .. } => {
            if investigate::candidates(state, investigator).is_none() {
                return Err(
                    "an Investigate ability needs a revealed location to investigate".into(),
                );
            }
            Ok(())
        }
        ActionDesignator::Parley | ActionDesignator::Resign | ActionDesignator::AutomaticEvade => {
            Ok(())
        }
        ActionDesignator::Evade { .. } => {
            if evade::candidates(state, investigator).is_empty() {
                return Err(
                    "an Evade ability needs an enemy engaged with you (none engaged)".into(),
                );
            }
            Ok(())
        }
        ActionDesignator::Move => Err(actions::designated_unimplemented_move()),
    }
}

#[cfg(test)]
mod tests {
    use card_dsl::dsl::{self, IntExpr};

    use super::*;
    use crate::state::{EnemyId, GameStateBuilder, LocationId};
    use crate::test_support::{self, seat, TEST_REGISTRIES};

    const ME: InvestigatorId = InvestigatorId(1);
    const HERE: LocationId = LocationId(1);

    fn fight() -> ActionDesignator {
        ActionDesignator::Fight {
            combat_modifier: IntExpr::Lit(1),
            extra_damage: IntExpr::Lit(0),
            skill_override: None,
            reveal_clause: None,
        }
    }

    fn investigate() -> ActionDesignator {
        ActionDesignator::Investigate {
            shroud_modifier: IntExpr::Lit(-2),
            instead_of_discovery: None,
        }
    }

    /// A board with the investigator at a revealed [`HERE`] and `enemies`
    /// co-located there.
    fn board(revealed: bool, enemies: u32) -> GameState {
        let mut loc = test_support::test_location(HERE.0, "Study");
        loc.revealed = revealed;
        let mut builder = GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(loc)
            .with_seat(seat(ME.0).at(HERE));
        for n in 0..enemies {
            let mut enemy = test_support::test_enemy(n + 1, "Ghoul");
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

    /// An evade needs an enemy *engaged with you*: one merely co-located, which
    /// a Fight could target, is no evade target.
    #[test]
    fn an_evade_needs_an_engaged_enemy() {
        let mut state = board(true, 2);
        assert!(can_perform(&state, ME, &dsl::evade()).is_err());
        state
            .enemies
            .get_mut(&EnemyId(2))
            .expect("the board's second enemy")
            .engaged_with = Some(ME);
        assert!(can_perform(&state, ME, &dsl::evade()).is_ok());
    }

    /// The designator no *implemented* card prints rejects rather than
    /// performing a guess — and it rejects *pre-cost*, so nothing is spent.
    #[test]
    fn an_unprinted_designator_rejects_rather_than_guessing() {
        let state = board(true, 1);
        assert!(can_perform(&state, ME, &ActionDesignator::Move).is_err());
    }
}
