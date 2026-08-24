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
//! Lives at `crates/cards/tests/` so it can install
//! [`cards::REGISTRY`] without colliding with `game-core`'s in-crate
//! tests (which deliberately don't install one and would see
//! `metadata_for == None`, contributing zero icons).

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, SkillKind, TokenModifiers, Zone,
};
use game_core::test_support::{
    drive_skill_test, perform_skill_test, test_investigator, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event, assert_event_count, assert_no_event};

const PERCEPTION: &str = "01090";
const UNEXPECTED_COURAGE: &str = "01093";
/// Overpower — `01091`, two combat icons and no wild. Used as the
/// "no appropriate icon for this test" control: ST.2 rejects it outright
/// on an intellect test (#763).
const OVERPOWER: &str = "01091";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Hand contents for the test. Builds a state with the named cards in
/// the active investigator's hand, base intellect 3, and a single-
/// `Numeric(0)` chaos bag (so the token-modifier contribution is
/// always 0).
fn state_with_hand(hand: &[&str]) -> (game_core::GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    // Cards for the draw-skills' success draws to land on. Without them the
    // draw hits an empty deck, which now applies the deck-out horror (#636) —
    // irrelevant noise for tests about icon contributions.
    inv.deck = vec![
        CardCode::new("spare-1"),
        CardCode::new("spare-2"),
        CardCode::new("spare-3"),
    ];
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id)
}

/// Start an Intellect-vs-5 plain skill test and drive it through with the
/// supplied commit codes. Uses `drive_skill_test` so the resolver can translate
/// codes → indices using the in-flight state at resolve time.
fn drive_with_commits(
    state: game_core::GameState,
    id: InvestigatorId,
    commit: &[&str],
) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    let codes: Vec<CardCode> = commit.iter().map(|c| CardCode::new(*c)).collect();
    resolver.commit_cards(&codes);
    drive_skill_test(state, id, SkillKind::Intellect, 5, resolver)
}

#[test]
fn empty_commit_against_difficulty_5_intellect_fails() {
    // Base 3 + 0 (token) + 0 (no commits) < 5 — fails by 2.
    let (state, id) = state_with_hand(&[PERCEPTION, UNEXPECTED_COURAGE]);
    let result = drive_with_commits(state, id, &[]);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 2, .. }
            if *investigator == id
    );
    // No commit → no discards.
    assert_no_event!(result.events, Event::CardDiscarded { .. });
}

#[test]
fn committing_perception_contributes_two_intellect_icons() {
    // Base 3 + 0 (token) + 2 (Perception's intellect icons) = 5,
    // meets difficulty 5 → success with margin 0.
    let (state, id) = state_with_hand(&[PERCEPTION, UNEXPECTED_COURAGE]);
    let result = drive_with_commits(state, id, &[PERCEPTION]);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    // Perception lands in discard; Unexpected Courage stays in hand, joined by
    // the card Perception's "If this test is successful, draw 1 card" drew.
    let inv = &result.state.investigators[&id];
    assert_eq!(
        inv.hand,
        vec![CardCode::new(UNEXPECTED_COURAGE), CardCode::new("spare-1")],
    );
    assert_eq!(inv.discard, vec![CardCode::new(PERCEPTION)]);
    assert_event!(
        result.events,
        Event::CardDiscarded { investigator, code, from: Zone::Hand }
            if *investigator == id && *code == CardCode::new(PERCEPTION)
    );
}

#[test]
fn committing_unexpected_courage_contributes_two_wild_icons() {
    // Wild icons count toward whichever skill the test is against.
    // Base 3 + 0 + 2 (UC's two wild) = 5 → success.
    let (state, id) = state_with_hand(&[PERCEPTION, UNEXPECTED_COURAGE]);
    let result = drive_with_commits(state, id, &[UNEXPECTED_COURAGE]);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.discard, vec![CardCode::new(UNEXPECTED_COURAGE)]);
}

#[test]
fn committing_two_cards_sums_both_contributions_and_discards_both() {
    // Phase-3 acceptance demo: commit two cards, verify icons
    // counted, both discarded after the test.
    //
    // Base 3 + 0 + 2 (Perception intellect) + 2 (UC wild) = 7 vs
    // difficulty 5 → success with margin 2.
    let (state, id) = state_with_hand(&[PERCEPTION, UNEXPECTED_COURAGE]);
    let result = drive_with_commits(state, id, &[PERCEPTION, UNEXPECTED_COURAGE]);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 2 }
            if *investigator == id
    );
    let inv = &result.state.investigators[&id];
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
    assert_event_count!(result.events, 2, Event::CardDiscarded { .. });
}

#[test]
fn mixing_matching_and_non_matching_commits_is_rejected() {
    // Perception (intellect 2) + Overpower (combat 2, no wild) against an
    // intellect test. ST.2 admits cards one at a time, and Overpower prints
    // no appropriate icon for this test, so the batch is rejected — the
    // matching card does not carry the non-matching one in with it (#763).
    // The icon *arithmetic* that used to live here is covered by
    // `committing_perception_contributes_two_intellect_icons`.
    let (state, id) = state_with_hand(&[PERCEPTION, OVERPOWER]);
    let result = drive_with_commits(state, id, &[PERCEPTION, OVERPOWER]);
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "expected Rejected, got {:?}",
        result.outcome,
    );
    let inv = &result.state.investigators[&id];
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
    let (state, id) = state_with_hand(&[OVERPOWER]);
    let result = drive_with_commits(state, id, &[OVERPOWER]);
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "expected Rejected, got {:?}",
        result.outcome,
    );
    // Nothing moved: the card is still in hand rather than in the discard.
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.hand, vec![CardCode::new(OVERPOWER)]);
    assert!(inv.discard.is_empty());
}

#[test]
fn awaiting_input_emits_between_started_and_revealed_for_real_card_state() {
    // Sanity check that the pause point lands in the right spot when
    // the registry is installed and the hand has real cards. First
    // `apply` returns AwaitingInput; the chaos token hasn't been
    // drawn yet.
    let (state, id) = state_with_hand(&[PERCEPTION]);
    let paused = perform_skill_test(state, id, SkillKind::Intellect, 5);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        paused.events,
        Event::SkillTestStarted { investigator, .. } if *investigator == id
    );
    assert_no_event!(paused.events, Event::ChaosTokenRevealed { .. });
}
