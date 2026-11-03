//! The group's clue spend on an `AdvanceAct`, through the public `apply` entry:
//! who is asked for a share, which amounts they are offered, and the clues left
//! once the act has advanced.
//!
//! `glossary/Act_Deck_and_Agenda_Deck.md`: *"Any or all investigators may
//! contribute any number of clues towards the total number of clues required to
//! advance the act."* The investigator taking the action covers whatever the
//! others leave, so only the others are ever asked.
//!
//! Synthetic acts only (ADR 0016): [`ACT_1`] models "an act advanced by
//! spending clues", its threshold set per test, and [`ACT_2`] its successor.

use game_core::action::InputResponse;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget, PromptNature, Responder};
use game_core::state::{GameStateBuilder, InvestigatorId};
use game_core::test_support::{act, seat, TestSession, TEST_REGISTRIES};

const ACTING: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const THIRD: InvestigatorId = InvestigatorId(3);
/// The act advanced, its clue threshold set per test.
const ACT_1: &str = "_ca_act";
/// The act after [`ACT_1`], so the advance moves the cursor.
const ACT_2: &str = "_ca_next_act";

/// [`ACTING`] taking `AdvanceAct` on [`ACT_1`] (threshold `threshold`), with
/// [`ACTING`] holding `acting` clues and [`OTHER`] `other`.
fn advance(threshold: u8, acting: u8, other: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).clues(acting))
        .with_seat(seat(2).clues(other))
        .with_seating([ACTING, OTHER])
        .open_turn(ACTING)
        .with_act_deck([act(ACT_1, threshold), act(ACT_2, 2)])
        .session()
        .take(&TurnAction::AdvanceAct {
            investigator: ACTING,
        })
}

fn clues(session: &TestSession, investigator: InvestigatorId) -> u8 {
    session.state().investigators[&investigator].clues
}

#[test]
fn the_other_investigator_is_asked_for_their_share_on_the_act() {
    let session = advance(4, 3, 3);
    let prompt = session.prompt();
    assert_eq!(prompt.responder, Responder::Investigator(OTHER));
    assert_eq!(prompt.target, Some(OptionTarget::Act));
    assert_eq!(prompt.nature, PromptNature::Decision);
    assert!(
        prompt.options.iter().all(|o| o.target.is_none()),
        "the amounts are not board entities"
    );
    // At least 1, since the acting investigator's 3 can't cover all 4; at
    // most the other investigator's 3.
    assert_eq!(
        session.offered_labels(),
        [
            "Contribute 1 clue",
            "Contribute 2 clues",
            "Contribute 3 clues"
        ]
    );
    assert_eq!(session.state().act_index, 0, "the act waits on the shares");
    assert_eq!(
        (clues(&session, ACTING), clues(&session, OTHER)),
        (3, 3),
        "nothing is spent while a share is undecided"
    );
}

#[test]
fn the_acting_investigator_covers_the_rest_unasked() {
    // The other investigator puts in 1, so the acting investigator gives 3.
    let session = advance(4, 3, 3).pick_nth(0);
    session.assert_at_turn_menu(ACTING);
    assert_eq!((clues(&session, ACTING), clues(&session, OTHER)), (0, 2));
    assert_eq!(session.state().act_index, 1);
}

#[test]
fn a_contributor_with_one_possible_share_is_not_asked() {
    // The acting investigator's 1 leaves 3 to the other investigator, who holds
    // exactly 3.
    let session = advance(4, 1, 3);
    session.assert_at_turn_menu(ACTING);
    assert_eq!((clues(&session, ACTING), clues(&session, OTHER)), (0, 0));
    assert_eq!(session.state().act_index, 1);
}

#[test]
fn a_group_holding_exactly_the_threshold_is_not_asked() {
    let session = advance(4, 2, 2);
    session.assert_at_turn_menu(ACTING);
    assert_eq!((clues(&session, ACTING), clues(&session, OTHER)), (0, 0));
    assert_eq!(session.state().act_index, 1);
}

#[test]
fn a_solo_investigator_is_not_asked() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).clues(5))
        .open_turn(ACTING)
        .with_act_deck([act(ACT_1, 3), act(ACT_2, 2)])
        .session()
        .take(&TurnAction::AdvanceAct {
            investigator: ACTING,
        });
    session.assert_at_turn_menu(ACTING);
    assert_eq!(clues(&session, ACTING), 2);
    assert_eq!(session.state().act_index, 1);
}

#[test]
fn a_contributor_holding_no_clues_is_not_asked() {
    let session = advance(3, 4, 0);
    session.assert_at_turn_menu(ACTING);
    assert_eq!((clues(&session, ACTING), clues(&session, OTHER)), (1, 0));
    assert_eq!(session.state().act_index, 1);
}

#[test]
fn a_share_answered_by_anyone_but_the_contributor_is_rejected() {
    advance(4, 3, 3)
        .assert_rejects(|s| s.answer_as(ACTING, InputResponse::PickSingle(OptionId(0))));
}

#[test]
fn a_share_that_was_not_offered_is_rejected() {
    advance(4, 3, 3).assert_rejects(|s| s.answer_as(OTHER, InputResponse::PickSingle(OptionId(3))));
}

/// With three seats, the investigators who did not take the action are asked
/// in player order, and the one taking it comes last.
#[test]
fn the_others_are_asked_in_player_order_before_the_acting_investigator() {
    // OTHER takes the action; ACTING and THIRD are asked, in that order.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).clues(2))
        .with_seat(seat(2).clues(2))
        .with_seat(seat(3).clues(2))
        .with_seating([ACTING, OTHER, THIRD])
        .open_turn(OTHER)
        .with_act_deck([act(ACT_1, 4), act(ACT_2, 2)])
        .session()
        .take(&TurnAction::AdvanceAct {
            investigator: OTHER,
        });
    assert_eq!(session.prompt().responder, Responder::Investigator(ACTING));
    // THIRD and OTHER after them hold 4, so ACTING may put in nothing.
    assert_eq!(
        session.offered_labels(),
        [
            "Contribute 0 clues",
            "Contribute 1 clue",
            "Contribute 2 clues"
        ]
    );
    let session = session.pick_nth(1);
    assert_eq!(session.prompt().responder, Responder::Investigator(THIRD));
    // 3 still needed, and OTHER after them holds 2.
    assert_eq!(
        session.offered_labels(),
        ["Contribute 1 clue", "Contribute 2 clues"]
    );
    let session = session.pick_nth(0);
    session.assert_at_turn_menu(OTHER);
    assert_eq!(
        (
            clues(&session, ACTING),
            clues(&session, THIRD),
            clues(&session, OTHER)
        ),
        (1, 1, 0)
    );
    assert_eq!(session.state().act_index, 1);
}
