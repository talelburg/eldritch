//! End-to-end: seated Roland Banks (01001) draws his `[elder_sign]` token
//! during a skill test → "+1 for each clue on your location" adds his
//! location's clue count to the total (0 / 1 / 2 clues).
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01001):
//! > [elder_sign] effect: +1 for each clue on your location.
//!
//! Integration test so it installs the real `cards::REGISTRY` (which carries
//! Roland's `Trigger::ElderSign` ability) in its own process.

use cards::REGISTRY;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, clues, seat, Seat};

const ROLAND: &str = "01001";
const COVER_UP: &str = "01007";
const INV: InvestigatorId = InvestigatorId(1);
const STUDY: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Roland, seated from his printed card (Willpower 3) at the Study and in
/// play only as his investigator card, so the bonus can come from nothing but
/// the investigator-card elder-sign bridge.
fn roland() -> Seat {
    seat(1).as_card(ROLAND).at(STUDY)
}

/// Run a Willpower test at difficulty 3 with the `ElderSign` token, `roland`
/// at the Study holding `clues_here`, committing nothing. Returns every event,
/// for outcome assertions.
fn run_elder_sign_test(clues_here: u8, roland: Seat) -> Vec<Event> {
    let mut study = test_support::test_location(10, "Study");
    study.clues = clues_here;

    // Willpower 3 vs difficulty 3. ElderSign bonus = clues at the location,
    // so total = 3 + clues: always a success, by margin = clues.
    let session = GameStateBuilder::new()
        .with_seat(roland)
        .with_location(study)
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, 3)
        .commit(&[]);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV)),
        "the test resolves and play is back at Roland's turn menu",
    );
    assert!(
        session.state().investigators[&INV].cards_in_play.is_empty(),
        "the bonus must come from the investigator-card elder-sign bridge \
         (investigator_card.code), not a played card",
    );
    session.events().to_vec()
}

#[test]
fn elder_sign_adds_zero_clues() {
    let events = run_elder_sign_test(0, roland());
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 0
        )),
        "0 clues → +0 → succeed by 0: {events:?}",
    );
}

#[test]
fn elder_sign_adds_one_clue() {
    let events = run_elder_sign_test(1, roland());
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 1
        )),
        "1 clue → +1 → succeed by 1: {events:?}",
    );
}

#[test]
fn elder_sign_adds_two_clues() {
    let events = run_elder_sign_test(2, roland());
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 2
        )),
        "2 clues → +2 → succeed by 2: {events:?}",
    );
}

/// Clues sitting on a *card* in the threat area are not clues "at" the
/// location, so the elder sign counts none of them.
///
/// The assertion is the same one `elder_sign_adds_zero_clues` makes; only the
/// fixture differs, and the fixture is the point. This is a regression guard
/// against a future `CluesAtControllerLocation` that sweeps threat-area or
/// in-play cards for clues alongside the location's own.
///
/// `data/official-faq/Frequently_Asked_Questions.md` asks exactly this pair —
/// Cover Up 01007 under Roland's `[elder_sign]`:
///
/// > Q: Are clues on Cover Up ([core] 7) considered to be "at my location" for
/// > the purposes of Roland's [elder_sign] ability?
/// >
/// > A: No. Generally speaking, cards (such as investigators, assets under your
/// > control, enemies in your threat area, etc) are "at" a location. Clues are
/// > only "at" a location if they are physically on that location ("Clues,"
/// > Rules Reference, page 7).
#[test]
fn clues_on_a_threat_area_card_are_not_clues_at_the_location() {
    // Cover Up's Revelation places 3 clues on itself.
    let events = run_elder_sign_test(0, roland().threat_area([(COVER_UP, clues(3))]));
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 0
        )),
        "3 clues on Cover Up in the threat area, 0 on the location → +0: {events:?}",
    );
}
