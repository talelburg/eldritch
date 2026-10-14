//! End-to-end test that Holy Rosary's +1 willpower applies during a
//! real skill test once the card is in play.
//!
//! This is the Phase-3 demonstration the project has been building
//! toward: a real card's `Trigger::Constant` ability contributes to
//! skill-test totals through the registry (#88), via the in-play
//! state populated by `PlayCard` (#89), summed by the constant-
//! modifier query (#92). Setup uses `PlayCard` to land the rosary in
//! `cards_in_play` so the action log mirrors a real session.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, TestSession};

const HOLY_ROSARY: &str = "01059";
const INV: InvestigatorId = InvestigatorId(1);

/// A session where the controller has Holy Rosary in hand, at their open
/// turn in the Investigation phase, has a non-empty chaos bag (a single Zero
/// token so the skill-test arithmetic is trivially `skill + 0 vs.
/// difficulty`), and starts with 3 willpower + 3 intellect from the fixture
/// defaults.
fn rosary_in_hand() -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand([HOLY_ROSARY]))
        .open_turn(INV)
        // Single-token bag → token-modifier is always 0; the skill
        // test outcome is decided entirely by the base + constant
        // contribution vs. difficulty.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
}

/// Play Holy Rosary out of hand; the turn goes on with it in play.
fn rosary_in_play() -> TestSession {
    let session = rosary_in_hand().take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    });
    session.assert_at_turn_menu(INV);
    session
}

/// Run a plain `skill` test against `difficulty` from the turn menu,
/// committing nothing; it comes back to the menu.
fn test_committing_nothing(session: TestSession, skill: SkillKind, difficulty: i8) -> TestSession {
    let session = session.test_skill(INV, skill, difficulty).commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn willpower_test_succeeds_at_difficulty_4_after_playing_holy_rosary() {
    // Test investigator has 3 willpower. Without the rosary, a
    // difficulty-4 willpower test fails (3 + 0 < 4). With the rosary
    // in play (+1 willpower constant), it succeeds (4 + 0 >= 4).
    let after_play = rosary_in_play();
    let in_play = &after_play.state().investigators[&INV].cards_in_play;
    assert_eq!(in_play.len(), 1);
    assert_eq!(in_play[0].code, CardCode::new(HOLY_ROSARY));

    // Difficulty-4 willpower test — +1 from the rosary should carry it.
    let result = test_committing_nothing(after_play, SkillKind::Willpower, 4);
    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Willpower, margin: 0 }
            if *investigator == INV
    );
}

#[test]
fn willpower_test_fails_at_difficulty_5_even_with_holy_rosary() {
    // +1 isn't a free pass: 3 + 1 + 0 < 5 still fails.
    let result = test_committing_nothing(rosary_in_play(), SkillKind::Willpower, 5);
    assert_event!(
        result.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Willpower, by: 1, .. }
            if *investigator == INV
    );
}

#[test]
fn intellect_test_unaffected_by_holy_rosary_in_play() {
    // Holy Rosary modifies Stat::Willpower, not Stat::Intellect.
    // A difficulty-4 intellect test fails (3 + 0 < 4) regardless of
    // the rosary being in play.
    let result = test_committing_nothing(rosary_in_play(), SkillKind::Intellect, 4);
    assert_event!(
        result.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == INV
    );
}

#[test]
fn willpower_test_without_rosary_in_play_uses_base_value_only() {
    // Belt-and-suspenders: with the rosary still in hand (not played),
    // a difficulty-4 willpower test fails just like the no-card case.
    // Confirms the modifier query reads cards_in_play, not hand.
    let session = rosary_in_hand();
    assert!(!session.state().investigators[&INV].hand.is_empty());
    assert!(session.state().investigators[&INV].cards_in_play.is_empty());

    let result = test_committing_nothing(session, SkillKind::Willpower, 4);
    assert_event!(
        result.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Willpower, by: 1, .. }
            if *investigator == INV
    );
}
