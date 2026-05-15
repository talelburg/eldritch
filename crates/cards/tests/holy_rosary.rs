//! End-to-end test that Holy Rosary's +1 willpower applies during a
//! real skill test once the card is in play.
//!
//! This is the Phase-3 demonstration the project has been building
//! toward: a real card's `Trigger::Constant` ability contributes to
//! skill-test totals through the registry (#88), via the in-play
//! state populated by `PlayCard` (#89), summed by the constant-
//! modifier query (#92). Setup uses `PlayCard` to land the rosary in
//! `cards_in_play` so the action log mirrors a real session.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{test_investigator, TestGame};
use game_core::{assert_event, Action, PlayerAction};

const HOLY_ROSARY: &str = "01059";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Build a state where the controller has Holy Rosary in hand, is the
/// active investigator in the Investigation phase, has a non-empty
/// chaos bag (a single Zero token so the skill-test arithmetic is
/// trivially `skill + 0 vs. difficulty`), and starts with 3 willpower
/// + 3 intellect from the fixture defaults.
fn state_with_rosary_in_hand() -> (game_core::GameState, InvestigatorId) {
    install_real_registry();

    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.hand = vec![CardCode::new(HOLY_ROSARY)];

    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        // Single-token bag → token-modifier is always 0; the skill
        // test outcome is decided entirely by the base + constant
        // contribution vs. difficulty.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id)
}

#[test]
fn willpower_test_succeeds_at_difficulty_4_after_playing_holy_rosary() {
    // Test investigator has 3 willpower. Without the rosary, a
    // difficulty-4 willpower test fails (3 + 0 < 4). With the rosary
    // in play (+1 willpower constant), it succeeds (4 + 0 >= 4).
    let (state, id) = state_with_rosary_in_hand();

    // Play Holy Rosary out of hand.
    let after_play = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: id,
            hand_index: 0,
        }),
    );
    assert_eq!(after_play.outcome, EngineOutcome::Done);
    assert_eq!(
        after_play.state.investigators[&id].cards_in_play,
        vec![CardCode::new(HOLY_ROSARY)],
    );

    // Difficulty-4 willpower test — +1 from the rosary should carry it.
    let result = apply(
        after_play.state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Willpower,
            difficulty: 4,
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, margin: 0 }
            if *investigator == id
    );
}

#[test]
fn willpower_test_fails_at_difficulty_5_even_with_holy_rosary() {
    // +1 isn't a free pass: 3 + 1 + 0 < 5 still fails.
    let (state, id) = state_with_rosary_in_hand();

    let after_play = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: id,
            hand_index: 0,
        }),
    );
    assert_eq!(after_play.outcome, EngineOutcome::Done);

    let result = apply(
        after_play.state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Willpower,
            difficulty: 5,
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Willpower, by: 1, .. }
            if *investigator == id
    );
}

#[test]
fn intellect_test_unaffected_by_holy_rosary_in_play() {
    // Holy Rosary modifies Stat::Willpower, not Stat::Intellect.
    // A difficulty-4 intellect test fails (3 + 0 < 4) regardless of
    // the rosary being in play.
    let (state, id) = state_with_rosary_in_hand();

    let after_play = apply(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: id,
            hand_index: 0,
        }),
    );
    assert_eq!(after_play.outcome, EngineOutcome::Done);

    let result = apply(
        after_play.state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Intellect,
            difficulty: 4,
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == id
    );
}

#[test]
fn willpower_test_without_rosary_in_play_uses_base_value_only() {
    // Belt-and-suspenders: with the rosary still in hand (not played),
    // a difficulty-4 willpower test fails just like the no-card case.
    // Confirms the modifier query reads cards_in_play, not hand.
    let (state, id) = state_with_rosary_in_hand();
    assert!(!state.investigators[&id].hand.is_empty());
    assert!(state.investigators[&id].cards_in_play.is_empty());

    let result = apply(
        state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Willpower,
            difficulty: 4,
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Willpower, by: 1, .. }
            if *investigator == id
    );
}
