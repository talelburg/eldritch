//! End-to-end test that Magnifying Glass's +1 intellect applies
//! during Investigate but not during a bare intellect test.
//!
//! The acceptance criterion from #45 (per-skill-test-kind scope) was
//! "Magnifying Glass's full impl lands and the simulator applies +1
//! intellect to investigate tests but not to other intellect tests."
//! This file closes that loop with the real card and real registry.

use cards::REGISTRY;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
    TokenModifiers,
};
use game_core::test_support::{self, seat, TestSession};

const MAGNIFYING_GLASS: &str = "01030";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

const PLAY_MAGNIFYING_GLASS: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    hand_index: 0,
};

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// A session at the controller's turn menu, with Magnifying Glass in hand,
/// placed at a 4-shroud location with a clue, and a single-Numeric(0) chaos
/// bag so the token modifier is always 0. Skill defaults (3 intellect) plus
/// the card's bonus (when in play) cleanly cross / miss difficulty 4 depending
/// on whether the bonus applies.
fn mg_in_hand() -> TestSession {
    let mut location = test_support::test_location(HERE.0, "Study");
    location.shroud = 4;
    location.clues = 1;

    GameStateBuilder::new()
        .with_location(location)
        .with_seat(seat(1).at(HERE).hand([MAGNIFYING_GLASS]))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

#[test]
fn investigate_succeeds_at_shroud_4_after_playing_magnifying_glass() {
    // 3 intellect + 1 (Magnifying Glass) + 0 (token) = 4 vs shroud 4 → succeed by 0.
    let after_play = mg_in_hand().take(&PLAY_MAGNIFYING_GLASS);
    after_play.assert_at_turn_menu(INV);
    assert_eq!(
        after_play.state().investigators[&INV].cards_in_play[0].code,
        CardCode::new(MAGNIFYING_GLASS),
    );

    let session = after_play
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.finish().events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
}

#[test]
fn investigate_fails_at_shroud_4_without_magnifying_glass_in_play() {
    // Same setup minus the card in play — 3 + 0 < 4 → fail by 1.
    let session = mg_in_hand();
    assert!(session.state().investigators[&INV].cards_in_play.is_empty());

    // Magnifying Glass is Fast and still in hand, so the test's player windows
    // offer it, before the commit window and after it; pass on both.
    let session = session
        .take(&TurnAction::Investigate { investigator: INV })
        .skip()
        .commit(&[])
        .skip();
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == INV
    );
}

#[test]
fn bare_intellect_test_unaffected_by_magnifying_glass_in_play() {
    // The bonus is gated to `SkillTestKind::Investigate`. A bare
    // intellect test (e.g. a treachery testing intellect) goes
    // through a plain skill test (`SkillTestKind::Plain`) —
    // the Magnifying Glass contribution must NOT apply. 3 + 0 < 4
    // → fail by 1, even with the card in play.
    let after_play = mg_in_hand().take(&PLAY_MAGNIFYING_GLASS);
    after_play.assert_at_turn_menu(INV);

    let session = after_play
        .test_skill(INV, SkillKind::Intellect, 4)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.finish().events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == INV
    );
}
