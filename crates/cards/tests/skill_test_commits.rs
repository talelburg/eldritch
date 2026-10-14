//! End-to-end test that real-card skill icons contribute to a skill
//! test's total when committed from hand.
//!
//! This is the Phase-3 acceptance demo for #63: commit Perception
//! (`01090`, two intellect icons) and/or Unexpected Courage (`01093`,
//! two wild icons) to a difficulty-5 intellect test for an
//! investigator with base intellect 3. The bag is a single
//! `Numeric(0)`, so the only thing that can push the total over the
//! line is the committed cards.
//!
//! Lives at `crates/cards/tests/` so it can run against
//! `cards::TEST_REGISTRIES`; `game-core`'s in-crate tests can't reach the
//! corpus and would see `metadata_for == None`, contributing zero icons.

use cards::TEST_REGISTRIES;
use game_core::engine::InputKind;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, DiscardPile, GameStateBuilder, InvestigatorId, SkillKind,
    TokenModifiers, Zone,
};
use game_core::test_support::{seat, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

const PERCEPTION: &str = "01090";
const UNEXPECTED_COURAGE: &str = "01093";
/// Overpower — `01091`, two combat icons and no wild. Used as the
/// "no appropriate icon for this test" control: ST.2 rejects it outright
/// on an intellect test (#763).
const OVERPOWER: &str = "01091";

const INV: InvestigatorId = InvestigatorId(1);

/// `INV`'s open turn with the named cards in hand, base intellect 3, and a
/// single-`Numeric(0)` chaos bag (so the token-modifier contribution is
/// always 0), started into an Intellect-vs-5 plain skill test. The session
/// rests at its commit window.
fn test_with_hand(hand: &[&str]) -> TestSession {
    // Cards for the draw-skills' success draws to land on. Without them the
    // draw hits an empty deck, which now applies the deck-out horror (#636) —
    // irrelevant noise for tests about icon contributions.
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .hand(hand.iter().copied())
                .deck(["spare-1", "spare-2", "spare-3"]),
        )
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Intellect, 5)
}

/// [`test_with_hand`], committing `commit`.
fn commit_from(hand: &[&str], commit: &[&str]) -> TestSession {
    test_with_hand(hand).commit(commit)
}

#[test]
fn empty_commit_against_difficulty_5_intellect_fails() {
    // Base 3 + 0 (token) + 0 (no commits) < 5 — fails by 2.
    let session = commit_from(&[PERCEPTION, UNEXPECTED_COURAGE], &[]);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 2, .. }
            if *investigator == INV
    );
    // No commit → no discards.
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
}

#[test]
fn committing_perception_contributes_two_intellect_icons() {
    // Base 3 + 0 (token) + 2 (Perception's intellect icons) = 5,
    // meets difficulty 5 → success with margin 0.
    let session = commit_from(&[PERCEPTION, UNEXPECTED_COURAGE], &[PERCEPTION]);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    // Perception lands in discard; Unexpected Courage stays in hand, joined by
    // the card Perception's "If this test is successful, draw 1 card" drew.
    let inv = &session.state().investigators[&INV];
    assert_eq!(
        inv.hand,
        vec![CardCode::new(UNEXPECTED_COURAGE), CardCode::new("spare-1")],
    );
    assert_eq!(inv.discard, vec![CardCode::new(PERCEPTION)]);
    assert_event!(
        session.events(),
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(investigator) }
            if *investigator == INV && *code == CardCode::new(PERCEPTION)
    );
}

#[test]
fn committing_unexpected_courage_contributes_two_wild_icons() {
    // Wild icons count toward whichever skill the test is against.
    // Base 3 + 0 + 2 (UC's two wild) = 5 → success.
    let session = commit_from(&[PERCEPTION, UNEXPECTED_COURAGE], &[UNEXPECTED_COURAGE]);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.discard, vec![CardCode::new(UNEXPECTED_COURAGE)]);
}

#[test]
fn committing_two_cards_sums_both_contributions_and_discards_both() {
    // Phase-3 acceptance demo: commit two cards, verify icons
    // counted, both discarded after the test.
    //
    // Base 3 + 0 + 2 (Perception intellect) + 2 (UC wild) = 7 vs
    // difficulty 5 → success with margin 2.
    let session = commit_from(
        &[PERCEPTION, UNEXPECTED_COURAGE],
        &[PERCEPTION, UNEXPECTED_COURAGE],
    );
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 2 }
            if *investigator == INV
    );
    let inv = &session.state().investigators[&INV];
    assert_eq!(
        inv.hand,
        vec![CardCode::new("spare-1")],
        "both cards removed from hand; Perception's success draw put one back",
    );
    // Both ended up in discard, in commit order — ST.8 empties the in-flight
    // record's limbo list, which holds the cards in the order they were
    // committed (the same order the ST.7 `OnCommit` effects fire in).
    assert_eq!(
        inv.discard,
        vec![CardCode::new(PERCEPTION), CardCode::new(UNEXPECTED_COURAGE),],
    );
    assert_event_count!(session.events(), 2, Event::CardDiscarded { .. });
}

#[test]
fn mixing_matching_and_non_matching_commits_is_rejected() {
    // Perception (intellect 2) + Overpower (combat 2, no wild) against an
    // intellect test. ST.2 admits cards one at a time, and Overpower prints
    // no appropriate icon for this test, so the batch is rejected — the
    // matching card does not carry the non-matching one in with it (#763).
    // The icon *arithmetic* that used to live here is covered by
    // `committing_perception_contributes_two_intellect_icons`.
    let session = commit_from(&[PERCEPTION, OVERPOWER], &[PERCEPTION, OVERPOWER]);
    session.expect_rejected();
    let inv = &session.state().investigators[&INV];
    assert_eq!(
        inv.hand,
        vec![CardCode::new(PERCEPTION), CardCode::new(OVERPOWER)],
        "a rejected commit leaves the hand untouched",
    );
    assert!(inv.discard.is_empty());
}

#[test]
fn committing_overpower_to_an_intellect_test_is_rejected() {
    // RR Appendix II ST.2: *"Cards that lack an appropriate skill icon may
    // not be committed to a skill test."* Overpower's two combat icons are
    // not appropriate for an intellect test and it prints no wild, so the
    // commit never happens — previously it was accepted as a zero-icon
    // commit, which made the commit window a free discard outlet (#763).
    let session = commit_from(&[OVERPOWER], &[OVERPOWER]);
    session.expect_rejected();
    // Nothing moved: the card is still in hand rather than in the discard.
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand, vec![CardCode::new(OVERPOWER)]);
    assert!(inv.discard.is_empty());
}

#[test]
fn awaiting_input_emits_between_started_and_revealed_for_real_card_state() {
    // Sanity check that the pause point lands in the right spot when
    // the registry is the real one and the hand has real cards. Starting the
    // test rests at the commit window; the chaos token hasn't been drawn yet.
    let paused = test_with_hand(&[PERCEPTION]);
    assert_eq!(paused.prompt().kind, InputKind::PickMultiple);
    assert_event!(
        paused.events(),
        Event::SkillTestStarted { investigator, .. } if *investigator == INV
    );
    assert_no_event!(paused.events(), Event::ChaosTokenRevealed { .. });
}
