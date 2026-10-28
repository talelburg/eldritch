//! Regression for #763 and #788: the commit window offers no card that prints
//! no appropriate skill icon, as RR Appendix II ST.2 forbids — *"An appropriate
//! skill icon is either one that matches the skill being tested, or a wild
//! icon. […] Cards that lack an appropriate skill icon may not be committed to
//! a skill test."*
//!
//! Offering anything in hand would make commit a free discard outlet (dump a
//! weakness).
//!
//! Driven against the real corpus (`cards::TEST_REGISTRIES`) so the icons under
//! test are the ones the pipeline actually compiled, not fabricated fixtures,
//! mirroring `commit_cap.rs`.

use cards::TEST_REGISTRIES;
use game_core::engine::{InputKind, OptionTarget, PickCount};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind,
};
use game_core::test_support::{seat, TestSession};

/// Overpower — two [combat] icons, no wild.
const OVERPOWER: &str = "01091";
/// Unexpected Courage — two [wild] icons.
const UNEXPECTED_COURAGE: &str = "01093";
/// Perception — two [intellect] icons.
const PERCEPTION: &str = "01090";
/// Emergency Cache — an Event with no printed skill icons at all.
const EMERGENCY_CACHE: &str = "01088";

const INV: InvestigatorId = InvestigatorId(1);

/// Drive a difficulty-1 test of `skill`, on the investigator's turn holding
/// `hand` with a deterministic bag, to the commit window.
fn at_commit_window(hand: &[&str], skill: SkillKind) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        // Somewhere for a success draw to land; an empty deck would apply the
        // deck-out horror, irrelevant noise here.
        .with_seat(
            seat(1)
                .hand(hand.iter().copied())
                .deck(["spare-1", "spare-2"]),
        )
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
        .test_skill(INV, skill, 1)
}

/// [`at_commit_window`], then commit `commits`.
fn commit_to(hand: &[&str], skill: SkillKind, commits: &[&str]) -> TestSession {
    at_commit_window(hand, skill).commit(commits)
}

/// The investigator's hand card at `hand_index`.
fn hand_card(hand_index: u8) -> OptionTarget {
    OptionTarget::HandCard {
        investigator: INV,
        hand_index,
    }
}

/// Assert `session`'s commit was accepted: the test resolved, play is back at
/// the turn menu, and the committed `card` was discarded.
fn assert_committed(session: &TestSession, card: &str) {
    session.assert_at_turn_menu(INV);
    assert!(!session.state().has_skill_test_in_flight());
    assert!(
        session.state().investigators[&INV]
            .discard
            .contains(&CardCode::new(card)),
        "the committed {card} resolved and was discarded",
    );
}

/// The commit window lists only the cards that may be committed (#788):
/// holding Perception and Overpower at an [intellect] test, it offers
/// Perception and leaves Overpower out, since Overpower's two [combat] icons
/// are not appropriate and it prints no wild.
#[test]
fn the_commit_window_offers_only_cards_with_an_appropriate_icon() {
    let session = at_commit_window(&[PERCEPTION, OVERPOWER], SkillKind::Intellect);
    let request = session.prompt();
    assert_eq!(
        request.options.len(),
        1,
        "only Perception is offered: {:?}",
        request.options
    );
    assert_eq!(
        session.offered_at(&hand_card(0)),
        1,
        "Perception is offered"
    );
    assert_eq!(
        session.offered_at(&hand_card(1)),
        0,
        "Overpower is not offered"
    );
    assert_eq!(
        request.kind,
        InputKind::PickMultiple(PickCount { min: 0, max: 1 }),
        "any number of the offered cards, none included",
    );
}

/// The free-discard hole stays shut: a card with no icons at all is not
/// offered to a test of any skill.
#[test]
fn a_card_with_no_icons_is_never_offered() {
    for skill in [
        SkillKind::Willpower,
        SkillKind::Intellect,
        SkillKind::Combat,
        SkillKind::Agility,
    ] {
        let session = at_commit_window(&[EMERGENCY_CACHE], skill);
        assert_eq!(
            session.offered_at(&hand_card(0)),
            0,
            "Emergency Cache is not offered to a {skill:?} test",
        );
    }
}

/// A matching icon is appropriate: Overpower to a [combat] test commits.
#[test]
fn matching_icon_card_commits() {
    let session = commit_to(&[OVERPOWER], SkillKind::Combat, &[OVERPOWER]);
    assert_committed(&session, OVERPOWER);
}

/// A wild icon is appropriate for every skill (glossary, *Wild Skill Icons*:
/// *"may be used to match any other skill icon"*).
#[test]
fn wild_icon_card_commits_to_any_skill() {
    for skill in [
        SkillKind::Willpower,
        SkillKind::Intellect,
        SkillKind::Combat,
        SkillKind::Agility,
    ] {
        let session = commit_to(&[UNEXPECTED_COURAGE], skill, &[UNEXPECTED_COURAGE]);
        assert_committed(&session, UNEXPECTED_COURAGE);
    }
}
