//! End-to-end: seated Roland Banks (01001) draws his `[elder_sign]` token
//! during a skill test → "+1 for each clue on your location" adds his
//! location's clue count to the total (0 / 1 / 2 clues).
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01001):
//! > [elder_sign] effect: +1 for each clue on your location.
//!
//! Integration test so it installs the real `cards::REGISTRY` (which carries
//! Roland's `Trigger::ElderSign` ability) in its own process.

use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase,
    SkillKind, TokenModifiers,
};
use game_core::test_support::{
    drive_skill_test, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::EngineOutcome;

const ROLAND: &str = "01001";
const COVER_UP: &str = "01007";

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Drive a Willpower-3 test at difficulty 3 with the `ElderSign` token, Roland
/// seated (`card_code` set, NOT in `cards_in_play`) at a location holding
/// `clues`. Returns the resolved events for outcome assertions.
fn run_elder_sign_test(clues: u8) -> Vec<Event> {
    run_elder_sign_test_with_threat_area(clues, Vec::new())
}

/// As [`run_elder_sign_test`], with `threat_area` pre-placed in Roland's threat
/// area — the fixture for "clues on a card are not clues at the location".
fn run_elder_sign_test_with_threat_area(clues: u8, threat_area: Vec<CardInPlay>) -> Vec<Event> {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    // After #448 cp2a the elder-sign scanner reads investigator_card.code, not card_code.
    inv.investigator_card.code = CardCode::new(ROLAND);
    inv.current_location = Some(loc_id);
    inv.skills.willpower = 3; // base 3
    assert!(
        inv.cards_in_play.is_empty(),
        "the bonus must come from the investigator-card elder-sign bridge (investigator_card.code), \
         not a played card — guard against a fixture change pre-populating cards_in_play",
    );
    inv.threat_area = threat_area;

    let mut loc = test_location(10, "Study");
    loc.clues = clues;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .with_token_modifiers(TokenModifiers::default())
        .build();

    // Bare PerformSkillTest: Willpower vs difficulty 3. ElderSign bonus = clues.
    // total = 3 + clues; succeed iff total >= 3 (always, here) by margin = clues.
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    let result = drive_skill_test(state, inv_id, SkillKind::Willpower, 3, resolver);
    assert_eq!(result.outcome, EngineOutcome::Done);
    result.events
}

#[test]
fn elder_sign_adds_zero_clues() {
    let events = run_elder_sign_test(0);
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
    let events = run_elder_sign_test(1);
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
    let events = run_elder_sign_test(2);
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
    let mut cover_up = CardInPlay::enter_play(CardCode::new(COVER_UP), CardInstanceId(1));
    cover_up.clues = 3; // Cover Up's Revelation places 3 clues on itself.

    let events = run_elder_sign_test_with_threat_area(0, vec![cover_up]);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 0
        )),
        "3 clues on Cover Up in the threat area, 0 on the location → +0: {events:?}",
    );
}
