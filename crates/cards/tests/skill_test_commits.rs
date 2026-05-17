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

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, SkillKind, TokenModifiers, Zone,
};
use game_core::test_support::{drive, test_investigator, ScriptedResolver, TestGame};
use game_core::{assert_event, assert_event_count, assert_no_event, Action, PlayerAction};

const PERCEPTION: &str = "01090";
const UNEXPECTED_COURAGE: &str = "01093";
/// Overpower — `01091`, two combat icons. Used as a "non-matching
/// icons contribute 0" control.
const OVERPOWER: &str = "01091";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Hand contents for the test. Builds a state with the named cards in
/// the active investigator's hand, base intellect 3, and a single-
/// `Numeric(0)` chaos bag (so the token-modifier contribution is
/// always 0).
fn state_with_hand(hand: &[&str]) -> (game_core::GameState, InvestigatorId) {
    install_real_registry();
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    let state = TestGame::new()
        .with_investigator(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id)
}

fn intellect_test_difficulty_5(id: InvestigatorId) -> Action {
    Action::Player(PlayerAction::PerformSkillTest {
        investigator: id,
        skill: SkillKind::Intellect,
        difficulty: 5,
    })
}

/// Drive one `PerformSkillTest` through with the supplied commit codes.
/// Uses `drive` directly so the resolver can translate codes →
/// indices using the in-flight state at resolve time.
fn drive_with_commits(
    state: game_core::GameState,
    action: Action,
    commit: &[&str],
) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    let codes: Vec<CardCode> = commit.iter().map(|c| CardCode::new(*c)).collect();
    resolver.commit_cards(&codes);
    drive(state, action, resolver)
}

#[test]
fn empty_commit_against_difficulty_5_intellect_fails() {
    // Base 3 + 0 (token) + 0 (no commits) < 5 — fails by 2.
    let (state, id) = state_with_hand(&[PERCEPTION, UNEXPECTED_COURAGE]);
    let result = drive_with_commits(state, intellect_test_difficulty_5(id), &[]);
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
    let result = drive_with_commits(state, intellect_test_difficulty_5(id), &[PERCEPTION]);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    // Perception lands in discard; Unexpected Courage stays in hand.
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.hand, vec![CardCode::new(UNEXPECTED_COURAGE)]);
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
    let result = drive_with_commits(
        state,
        intellect_test_difficulty_5(id),
        &[UNEXPECTED_COURAGE],
    );
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
    let result = drive_with_commits(
        state,
        intellect_test_difficulty_5(id),
        &[PERCEPTION, UNEXPECTED_COURAGE],
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 2 }
            if *investigator == id
    );
    let inv = &result.state.investigators[&id];
    assert!(inv.hand.is_empty(), "both cards removed from hand");
    // Both ended up in discard. (Order: descending-index removal, so
    // index 1 (UC) goes first, then index 0 (Perception). discard is
    // pushed back in that order.)
    assert_eq!(
        inv.discard,
        vec![CardCode::new(UNEXPECTED_COURAGE), CardCode::new(PERCEPTION),],
    );
    assert_event_count!(result.events, 2, Event::CardDiscarded { .. });
}

#[test]
fn mixing_matching_and_non_matching_commits_only_counts_the_matching_card() {
    // Commit Perception (intellect 2) + Overpower (combat 2, no
    // wild) together against an intellect test. Only Perception's
    // icons contribute: 3 + 0 + 2 + 0 = 5 vs difficulty 5 → success
    // with margin 0. Both cards still discard regardless of
    // contribution.
    let (state, id) = state_with_hand(&[PERCEPTION, OVERPOWER]);
    let result = drive_with_commits(
        state,
        intellect_test_difficulty_5(id),
        &[PERCEPTION, OVERPOWER],
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    let inv = &result.state.investigators[&id];
    assert!(inv.hand.is_empty(), "both cards removed from hand");
    assert_eq!(
        inv.discard,
        vec![CardCode::new(OVERPOWER), CardCode::new(PERCEPTION)],
    );
}

#[test]
fn committing_overpower_to_an_intellect_test_contributes_zero_icons() {
    // Non-matching skill icons + no wild = 0 contribution. Overpower
    // has two combat icons; committing it to an intellect test adds
    // nothing, so 3 + 0 + 0 = 3 < 5 still fails by 2.
    let (state, id) = state_with_hand(&[OVERPOWER]);
    let result = drive_with_commits(state, intellect_test_difficulty_5(id), &[OVERPOWER]);
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 2, .. }
            if *investigator == id
    );
    // The committed card still discards even though it contributed
    // nothing — commit-then-discard is the rule, regardless of
    // outcome or contribution.
    let inv = &result.state.investigators[&id];
    assert!(inv.hand.is_empty());
    assert_eq!(inv.discard, vec![CardCode::new(OVERPOWER)]);
}

#[test]
fn awaiting_input_emits_between_started_and_revealed_for_real_card_state() {
    // Sanity check that the pause point lands in the right spot when
    // the registry is installed and the hand has real cards. First
    // `apply` returns AwaitingInput; the chaos token hasn't been
    // drawn yet.
    let (state, id) = state_with_hand(&[PERCEPTION]);
    let paused = apply(state, intellect_test_difficulty_5(id));
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
