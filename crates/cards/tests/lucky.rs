//! Lucky! 01080 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01080):
//!
//! > Fast. Play when you would fail a skill test.
//! > Get +2 to your skill value for that test.
//!
//! Rulings (<https://arkhamdb.com/card/01080>):
//!
//! > The "When you would fail a skill test" triggering condition comes before
//! > the "After you fail a skill test" triggering condition, so you cannot play
//! > Lucky! after you use Rabbit’s Foot or play "Look what I found!".
//!
//! > If you reveal an \[auto_fail\] auto-fail token, your total skill value is
//! > considered to be 0, overriding any modifiers.
//!
//! The after-failure cards here are the real Rabbit's Foot 01075 and "Look what
//! I found!" 01079. Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::{Event, FailureReason};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
    TokenModifiers,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

const LUCKY: &str = "01080";
const LOOK_WHAT_I_FOUND: &str = "01079";
const RABBITS_FOOT: &str = "01075";
/// The deck's card, for Rabbit's Foot to draw.
const LEATHER_COAT: &str = "01072";

const INV: InvestigatorId = InvestigatorId(1);
/// A shroud-4 Study with 3 clues.
const HERE: LocationId = LocationId(10);

/// Investigator 1 at a shroud-4 Study, intellect `intellect`, 5 resources,
/// `hand` in hand and Rabbit's Foot in play, on their turn, with a lone `token`
/// in the bag.
fn board(intellect: i8, hand: &[&str], token: ChaosToken) -> TestSession {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.shroud = 4;
    study.clues = 3;
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, intellect)
                .resources(5)
                .hand(hand.iter().copied())
                .deck([LEATHER_COAT])
                .in_play([RABBITS_FOOT]),
        )
        .with_location(study)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

fn in_hand(code: &str) -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(code),
    }
}

fn investigate(session: TestSession) -> TestSession {
    session
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[])
}

fn copies_in_hand(session: &TestSession, code: &str) -> usize {
    session.state().investigators[&INV]
        .hand
        .iter()
        .filter(|c| **c == CardCode::new(code))
        .count()
}

#[test]
fn turning_a_failure_into_a_success_means_no_after_failure_card_sees_it() {
    // Intellect 3 against shroud 4 fails by 1; Lucky! makes it 5, a success.
    let window = investigate(board(
        3,
        &[LUCKY, LOOK_WHAT_I_FOUND],
        ChaosToken::Numeric(0),
    ));
    assert_eq!(window.offered_at(&in_hand(LUCKY)), 1, "{:?}", window.prompt());
    let foot = window.state().card(INV, RABBITS_FOOT);
    assert_eq!(window.offered_at(&foot), 0, "Rabbit's Foot is an after-cell card");

    let done = window.pick(in_hand(LUCKY));
    done.assert_at_turn_menu(INV);
    assert_no_event!(done.events(), Event::SkillTestFailed { .. });
    assert_event!(
        done.events(),
        Event::SkillTestSucceeded { investigator, margin: 1, .. } if *investigator == INV
    );
    assert_eq!(done.state().investigators[&INV].clues, 1, "the investigation succeeded");
    assert_eq!(copies_in_hand(&done, LOOK_WHAT_I_FOUND), 1, "never offered");
    assert!(
        done.state().investigators[&INV].hand.len() == 1,
        "Rabbit's Foot drew nothing: {:?}",
        done.state().investigators[&INV].hand
    );
    assert_eq!(done.state().investigators[&INV].resources, 4, "Lucky! cost 1");
}

#[test]
fn after_an_automatic_failure_it_changes_nothing() {
    let window = investigate(board(3, &[LUCKY], ChaosToken::AutoFail));
    assert_eq!(window.offered_at(&in_hand(LUCKY)), 1, "{:?}", window.prompt());

    let after = window.pick(in_hand(LUCKY));
    assert_no_event!(after.events(), Event::SkillTestSucceeded { .. });
    assert_event!(
        after.events(),
        Event::SkillTestFailed { reason: FailureReason::AutoFail, by: 4, .. }
    );
    assert_eq!(copies_in_hand(&after, LUCKY), 0, "Lucky! was played");
    let foot = after.state().card(INV, RABBITS_FOOT);
    assert_eq!(after.offered_at(&foot), 1, "the failure stands for the after cell");
}

#[test]
fn its_bonus_is_folded_before_the_clamp_at_zero() {
    // `glossary/Modifiers.md`: "base skill 4, –8 from chaos token, +2 from
    // "Lucky!" for a total of –2, which is still treated as zero." Against a
    // difficulty of 2, a clamp before the +2 would make the total 2 and pass.
    let window = board(4, &[LUCKY], ChaosToken::Numeric(-8))
        .test_skill(INV, SkillKind::Intellect, 2)
        .commit(&[]);
    let after = window.pick(in_hand(LUCKY));
    assert_no_event!(after.events(), Event::SkillTestSucceeded { .. });
    assert_event!(
        after.events(),
        Event::SkillTestFailed { reason: FailureReason::Total, by: 2, .. }
    );
}

#[test]
fn two_can_be_played_while_the_test_still_fails() {
    // Intellect 3 against 6 fails by 3: the first Lucky! leaves it failing by
    // 1, so the second is still eligible and makes it a success by 1.
    let window = board(3, &[LUCKY, LUCKY], ChaosToken::Numeric(0))
        .test_skill(INV, SkillKind::Intellect, 6)
        .commit(&[]);
    assert_eq!(window.offered_at(&in_hand(LUCKY)), 2, "{:?}", window.prompt());

    let window = window.pick_nth_at(in_hand(LUCKY), 0);
    assert_eq!(window.offered_at(&in_hand(LUCKY)), 1, "{:?}", window.prompt());

    let done = window.pick(in_hand(LUCKY));
    done.assert_at_turn_menu(INV);
    assert_no_event!(done.events(), Event::SkillTestFailed { .. });
    assert_event!(done.events(), Event::SkillTestSucceeded { margin: 1, .. });
    assert_eq!(copies_in_hand(&done, LUCKY), 0);
}

#[test]
fn a_second_is_withdrawn_once_the_first_has_made_the_test_succeed() {
    // Intellect 3 against 4 fails by 1: one Lucky! is enough, and "you would
    // fail" no longer holds for the second.
    let window = board(3, &[LUCKY, LUCKY], ChaosToken::Numeric(0))
        .test_skill(INV, SkillKind::Intellect, 4)
        .commit(&[]);
    assert_eq!(window.offered_at(&in_hand(LUCKY)), 2, "{:?}", window.prompt());

    let done = window.pick_nth_at(in_hand(LUCKY), 0);
    done.assert_at_turn_menu(INV);
    assert_event!(done.events(), Event::SkillTestSucceeded { margin: 1, .. });
    assert_event!(
        done.events(),
        Event::ReactionOptionLapsed { code, .. } if *code == CardCode::new(LUCKY)
    );
    assert_eq!(copies_in_hand(&done, LUCKY), 1, "the second stays in hand");
    assert_eq!(done.state().investigators[&INV].resources, 4, "one paid for");
}

#[test]
fn a_bonus_that_leaves_the_test_failing_narrows_the_margin_after_cells_read() {
    // Intellect 0 against shroud 4 fails by 4, too far for "Look what I
    // found!"; Lucky! makes it a failure by 2, which it is offered for.
    let window = investigate(board(
        0,
        &[LUCKY, LOOK_WHAT_I_FOUND],
        ChaosToken::Numeric(0),
    ));
    assert_eq!(window.offered_at(&in_hand(LOOK_WHAT_I_FOUND)), 0);

    let after = window.pick(in_hand(LUCKY));
    assert_event_count!(after.events(), 1, Event::SkillTestFailed { .. });
    assert_event!(after.events(), Event::SkillTestFailed { by: 2, .. });
    assert_eq!(
        after.offered_at(&in_hand(LOOK_WHAT_I_FOUND)),
        1,
        "{:?}",
        after.prompt()
    );
    let done = after.pick(in_hand(LOOK_WHAT_I_FOUND));
    assert_eq!(done.state().investigators[&INV].clues, 2);
}
