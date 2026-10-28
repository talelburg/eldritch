//! Regression for #763: the commit window rejects a card that prints no
//! appropriate skill icon, as RR Appendix II ST.2 forbids — *"An appropriate
//! skill icon is either one that matches the skill being tested, or a wild
//! icon. […] Cards that lack an appropriate skill icon may not be committed to
//! a skill test."*
//!
//! Accepting anything in hand would make commit a free discard outlet (dump a
//! weakness).
//!
//! Driven against the real corpus (`cards::TEST_REGISTRIES`) so the icons under
//! test are the ones the pipeline actually compiled, not fabricated fixtures,
//! mirroring `commit_cap.rs`.

use cards::TEST_REGISTRIES;
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
/// `hand` with a deterministic bag, to the commit window and commit `commits`.
fn commit_to(hand: &[&str], skill: SkillKind, commits: &[&str]) -> TestSession {
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
        .commit(commits)
}

/// Assert `session`'s commit was rejected and left the hand and the in-flight
/// test exactly as they were (validate-first).
fn assert_rejected_with_hand_intact(session: &TestSession, hand: &[&str]) {
    session.expect_rejected();
    let inv = &session.state().investigators[&INV];
    assert_eq!(
        inv.hand,
        hand.iter().map(|c| CardCode::new(*c)).collect::<Vec<_>>(),
        "a rejected commit must leave the hand untouched",
    );
    assert!(inv.discard.is_empty(), "a rejected commit discards nothing");
    assert!(
        session.state().has_skill_test_in_flight(),
        "the test stays paused at the commit window so the client can retry",
    );
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

/// Overpower's two [combat] icons are not appropriate for an [intellect]
/// test, and it prints no wild.
#[test]
fn non_matching_icon_commit_is_rejected() {
    let session = commit_to(&[OVERPOWER], SkillKind::Intellect, &[OVERPOWER]);
    assert_rejected_with_hand_intact(&session, &[OVERPOWER]);
}

/// The free-discard hole: a card with no icons at all cannot be committed.
#[test]
fn zero_icon_card_commit_is_rejected() {
    let session = commit_to(&[EMERGENCY_CACHE], SkillKind::Intellect, &[EMERGENCY_CACHE]);
    assert_rejected_with_hand_intact(&session, &[EMERGENCY_CACHE]);
}

/// One bad card rejects the whole batch — the commit is a single submission,
/// and validate-first means none of it lands.
#[test]
fn one_iconless_card_rejects_the_whole_batch() {
    let hand = [PERCEPTION, OVERPOWER];
    let session = commit_to(&hand, SkillKind::Intellect, &hand);
    assert_rejected_with_hand_intact(&session, &hand);
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
