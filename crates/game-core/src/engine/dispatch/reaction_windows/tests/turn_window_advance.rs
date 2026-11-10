//! The turn's player window offers the act advance to every investigator: the
//! window after 2.2 and after each action offers "Advance act" to every Active
//! investigator whenever the group affords it, and no other window does. The
//! rules behind it are quoted on `push_turn_window_advances`.

use super::*;
use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::Responder;
use crate::engine::OptionTarget;
use crate::state::{FastWindowKind, GameStateBuilder, InvestigatorId, Phase, PhaseStep};
use crate::test_support::{self, TestSession, TEST_REGISTRIES, TEST_REGISTRY};

const FIRST: InvestigatorId = InvestigatorId(1);
const SECOND: InvestigatorId = InvestigatorId(2);

/// Two investigators seated `[FIRST, SECOND]` holding `first_clues` and
/// `second_clues`, FIRST's turn open, under two stock acts of 2 clues each (two,
/// so the first is non-terminal).
fn two_seats(first_clues: u8, second_clues: u8) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(FIRST.0).clues(first_clues))
        .with_seat(test_support::seat(SECOND.0).clues(second_clues))
        .with_seating([FIRST, SECOND])
        .open_turn(FIRST)
        .with_act_deck([
            test_support::act("_test_act_1", 2),
            test_support::act("_test_act_2", 2),
        ])
}

/// Asserts the window's options are the act's advance, once for each
/// investigator: nobody seated here holds a fast play.
fn assert_advance_offered_to_both(session: &TestSession) {
    assert_eq!(session.offered_at(&OptionTarget::Act), 2);
    assert_eq!(session.offered_to(), [Some(FIRST), Some(SECOND)]);
}

#[test]
fn an_affordable_act_opens_the_window_after_an_action_offering_its_advance_to_everyone() {
    // Nobody holds a fast play: the affordable advance alone opens the window.
    let session = two_seats(1, 1).session().take(&TurnAction::Resource {
        investigator: FIRST,
    });

    assert_eq!(session.prompt().responder, Responder::Several);
    assert_advance_offered_to_both(&session);
}

#[test]
fn the_window_after_2_2_offers_the_advance() {
    // FIRST ends their turn; SECOND's turn begins and its window opens.
    let session = two_seats(1, 1).session().take(&TurnAction::EndTurn);

    assert_advance_offered_to_both(&session);
}

#[test]
fn an_investigator_with_no_clues_is_offered_the_advance() {
    let session = two_seats(2, 0).session().take(&TurnAction::Resource {
        investigator: FIRST,
    });

    assert_advance_offered_to_both(&session);
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
/// FIRST is asked for a share first, and SECOND covers the rest unasked.
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

/// A pass made before the advance lapses once it resolves: the window comes
/// back offering the next act's advance to both investigators, the one who
/// passed included.
#[test]
fn the_window_comes_back_after_the_advance_with_its_passes_cleared() {
    // 4 clues: 2 for each act.
    let session = two_seats(2, 2)
        .session()
        .take(&TurnAction::Resource {
            investigator: FIRST,
        })
        .skip_for(FIRST);
    assert_eq!(session.offered_to(), [Some(SECOND)]);

    // FIRST gives none of their 2, so SECOND's 2 pay for the first act and
    // FIRST's 2 still afford the next.
    let session = session.pick_for(SECOND, OptionTarget::Act).pick_nth(0);
    assert_eq!(session.state().act_index, 1);
    assert_advance_offered_to_both(&session);
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
