//! #763 integration: RR Appendix II ST.2 forbids committing a card that
//! prints no appropriate skill icon — *"An appropriate skill icon is either
//! one that matches the skill being tested, or a wild icon. […] Cards that
//! lack an appropriate skill icon may not be committed to a skill test."*
//!
//! Before this, the commit window accepted anything in hand, which made
//! commit a free discard outlet (dump a weakness, cf. #646).
//!
//! Driven against the real `cards::REGISTRY` so the icons under test are the
//! ones the pipeline actually compiled, not fabricated fixtures. Own process →
//! installs the registry, mirroring `commit_cap.rs`.

use game_core::engine::{EngineOutcome, OptionId};
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{perform_skill_test, test_investigator, GameStateBuilder};
use game_core::{Action, GameState, InputResponse, PlayerAction};

/// Overpower — two [combat] icons, no wild.
const OVERPOWER: &str = "01091";
/// Unexpected Courage — two [wild] icons.
const UNEXPECTED_COURAGE: &str = "01093";
/// Perception — two [intellect] icons.
const PERCEPTION: &str = "01090";
/// Emergency Cache — an Event with no printed skill icons at all.
const EMERGENCY_CACHE: &str = "01088";

const INV: InvestigatorId = InvestigatorId(1);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Investigator holding `hand`, mid-Investigation, with a deterministic bag.
fn board(hand: &[&str]) -> GameState {
    let mut inv = test_investigator(1);
    inv.hand = hand.iter().map(|c| CardCode::new(*c)).collect();
    // Somewhere for a success draw to land; an empty deck would apply the
    // deck-out horror (#636), irrelevant noise here.
    inv.deck = vec![CardCode::new("spare-1"), CardCode::new("spare-2")];
    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn commit(indices: Vec<u32>) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: indices.into_iter().map(OptionId).collect(),
        },
    })
}

/// Drive a difficulty-1 test of `skill` to the commit window and answer it
/// with `indices`.
fn commit_to(hand: &[&str], skill: SkillKind, indices: Vec<u32>) -> game_core::ApplyResult {
    let paused = perform_skill_test(board(hand), INV, skill, 1);
    assert!(
        matches!(paused.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected the commit window, got {:?}",
        paused.outcome,
    );
    game_core::engine::apply(paused.state, commit(indices))
}

/// Assert `result` rejected the commit and left the hand and the in-flight
/// test exactly as they were (validate-first).
fn assert_rejected_with_hand_intact(result: &game_core::ApplyResult, hand: &[&str]) {
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "expected Rejected, got {:?}",
        result.outcome,
    );
    let inv = &result.state.investigators[&INV];
    assert_eq!(
        inv.hand,
        hand.iter().map(|c| CardCode::new(*c)).collect::<Vec<_>>(),
        "a rejected commit must leave the hand untouched",
    );
    assert!(inv.discard.is_empty(), "a rejected commit discards nothing");
    assert!(
        result.state.has_skill_test_in_flight(),
        "the test stays paused at the commit window so the client can retry",
    );
}

/// Overpower's two [combat] icons are not appropriate for an [intellect]
/// test, and it prints no wild.
#[test]
fn non_matching_icon_commit_is_rejected() {
    let result = commit_to(&[OVERPOWER], SkillKind::Intellect, vec![0]);
    assert_rejected_with_hand_intact(&result, &[OVERPOWER]);
}

/// The free-discard hole: a card with no icons at all cannot be committed.
#[test]
fn zero_icon_card_commit_is_rejected() {
    let result = commit_to(&[EMERGENCY_CACHE], SkillKind::Intellect, vec![0]);
    assert_rejected_with_hand_intact(&result, &[EMERGENCY_CACHE]);
}

/// One bad card rejects the whole batch — the commit is a single submission,
/// and validate-first means none of it lands.
#[test]
fn one_iconless_card_rejects_the_whole_batch() {
    let hand = [PERCEPTION, OVERPOWER];
    let result = commit_to(&hand, SkillKind::Intellect, vec![0, 1]);
    assert_rejected_with_hand_intact(&result, &hand);
}

/// A matching icon is appropriate: Overpower to a [combat] test commits.
#[test]
fn matching_icon_card_commits() {
    let result = commit_to(&[OVERPOWER], SkillKind::Combat, vec![0]);
    assert_eq!(result.outcome, EngineOutcome::Done);
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
        let result = commit_to(&[UNEXPECTED_COURAGE], skill, vec![0]);
        assert_eq!(
            result.outcome,
            EngineOutcome::Done,
            "Unexpected Courage's wild icons must commit to a {skill:?} test",
        );
    }
}
