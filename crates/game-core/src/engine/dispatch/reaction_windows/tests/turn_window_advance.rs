//! The turn's player window offers the act advance to every investigator.
//!
//! `glossary/Clues.md`: *"during any investigator's turn the investigators may,
//! as a group, spend the requisite number of clues (usually conveyed as a "per
//! investigator" value) from their investigator cards to advance the act deck.
//! This is normally done as a [free] player ability."* `glossary/Ability.md`:
//! *"A [free] triggered ability may be triggered as a player ability during any
//! player window."* So the window after 2.2 and after each action offers
//! "Advance act" to every Active investigator whenever the group affords it
//! (#1328). The skill-test windows are excluded by decision, and the 2.1 window
//! comes before any turn.

use super::*;
use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::Responder;
use crate::engine::OptionTarget;
use crate::state::{FastWindowKind, GameStateBuilder, InvestigatorId, Phase, PhaseStep};
use crate::test_support::{self, act, TestSession, TEST_REGISTRIES, TEST_REGISTRY};

const FIRST: InvestigatorId = InvestigatorId(1);
const SECOND: InvestigatorId = InvestigatorId(2);

/// Two investigators seated `[FIRST, SECOND]` holding `first_clues` and
/// `second_clues`, FIRST's turn open, under two stock acts of 2 clues each (two,
/// so the first is non-terminal).
fn two_seats(first_clues: u8, second_clues: u8) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).clues(first_clues))
        .with_seat(test_support::seat(2).clues(second_clues))
        .with_seating([FIRST, SECOND])
        .open_turn(FIRST)
        .with_act_deck([act("_test_act_1", 2), act("_test_act_2", 2)])
}

/// The investigators the current prompt offers an option anchored to the act.
fn offered_the_advance(session: &TestSession) -> Vec<Option<InvestigatorId>> {
    session
        .prompt()
        .options
        .iter()
        .filter(|o| o.target == Some(OptionTarget::Act))
        .map(|o| o.investigator)
        .collect()
}

#[test]
fn an_affordable_act_opens_the_window_after_an_action_offering_its_advance_to_everyone() {
    // Nobody holds a fast play: the affordable advance alone opens the window.
    let session = two_seats(1, 1).session().take(&TurnAction::Resource {
        investigator: FIRST,
    });

    assert_eq!(session.prompt().responder, Responder::Several);
    assert_eq!(offered_the_advance(&session), [Some(FIRST), Some(SECOND)]);
}

#[test]
fn the_window_after_2_2_offers_the_advance() {
    // FIRST ends their turn; SECOND's turn begins and its window opens.
    let session = two_seats(1, 1).session().take(&TurnAction::EndTurn);

    assert_eq!(offered_the_advance(&session), [Some(FIRST), Some(SECOND)]);
}

#[test]
fn an_investigator_with_no_clues_is_offered_the_advance() {
    let session = two_seats(2, 0).session().take(&TurnAction::Resource {
        investigator: FIRST,
    });

    assert_eq!(offered_the_advance(&session), [Some(FIRST), Some(SECOND)]);
}

#[test]
fn an_unaffordable_act_leaves_the_window_shut() {
    let session = two_seats(1, 0).session().take(&TurnAction::Resource {
        investigator: FIRST,
    });

    session.assert_at_turn_menu(FIRST);
}

#[test]
fn the_advance_can_be_passed() {
    let session = two_seats(1, 1)
        .session()
        .take(&TurnAction::Resource {
            investigator: FIRST,
        })
        .skip();

    session.assert_at_turn_menu(FIRST);
    assert_eq!(session.state().act_index, 0);
}

/// SECOND starts the advance from FIRST's window, so SECOND is the initiator:
/// FIRST is asked for a share first, and SECOND covers the rest unasked (#153).
#[test]
fn an_investigator_starting_the_advance_from_the_window_is_its_initiator() {
    let session = two_seats(2, 1)
        .session()
        .take(&TurnAction::Resource {
            investigator: FIRST,
        })
        .pick_for(SECOND, OptionTarget::Act);

    assert_eq!(session.prompt().responder, Responder::Investigator(FIRST));
    assert_eq!(
        session.offered_labels(),
        ["Contribute 1 clue", "Contribute 2 clues"]
    );

    // FIRST gives 1; SECOND's 1 covers the rest. The next act is unaffordable,
    // so the window closes and FIRST's turn goes on.
    let session = session.pick_nth(0);
    session.assert_at_turn_menu(FIRST);
    let state = session.state();
    assert_eq!(state.investigators[&FIRST].clues, 1);
    assert_eq!(state.investigators[&SECOND].clues, 0);
    assert_eq!(state.act_index, 1);
}

/// The enumeration the window and its eligibility gate share, read with each
/// window kind on top of an affordable act.
fn advances_enumerated_in(phase: Phase, kind: FastWindowKind) -> Vec<TurnAction> {
    let state = two_seats(1, 1)
        .with_phase(phase)
        .with_open_window(kind)
        .build();
    enumerate_fast_plays(&state, &TEST_REGISTRY)
        .into_iter()
        .filter(|a| matches!(a, TurnAction::AdvanceAct { .. }))
        .collect()
}

#[test]
fn the_turn_window_enumerates_one_advance_per_investigator() {
    assert_eq!(
        advances_enumerated_in(
            Phase::Investigation,
            FastWindowKind::Phase(PhaseStep::TurnPlayerWindow)
        ),
        [
            TurnAction::AdvanceAct {
                investigator: FIRST
            },
            TurnAction::AdvanceAct {
                investigator: SECOND
            },
        ]
    );
}

#[test]
fn no_other_window_enumerates_the_advance() {
    for (phase, kind) in [
        (
            Phase::Investigation,
            FastWindowKind::Phase(PhaseStep::InvestigationBegins),
        ),
        (
            Phase::Investigation,
            FastWindowKind::SkillTest {
                before_token: false,
            },
        ),
        (
            Phase::Investigation,
            FastWindowKind::SkillTest { before_token: true },
        ),
        (
            Phase::Mythos,
            FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
        ),
    ] {
        assert_eq!(
            advances_enumerated_in(phase, kind),
            [],
            "{kind:?} must not offer the advance"
        );
    }
}
