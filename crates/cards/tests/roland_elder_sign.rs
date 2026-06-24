//! End-to-end: seated Roland Banks (01001) draws his `[elder_sign]` token
//! during a skill test → "+1 for each clue on your location" adds his
//! location's clue count to the total (0 / 1 / 2 clues).
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01001):
//! > [elder_sign] effect: +1 for each clue on your location.
//!
//! Integration test so it installs the real `cards::REGISTRY` (which carries
//! Roland's `Trigger::ElderSign` ability) in its own process.

use std::sync::Once;

use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{Action, EngineOutcome, PlayerAction};

const ROLAND: &str = "01001";

fn install_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Drive a Willpower-3 test at difficulty 3 with the `ElderSign` token, Roland
/// seated (`card_code` set, NOT in `cards_in_play`) at a location holding
/// `clues`. Returns the resolved events for outcome assertions.
fn run_elder_sign_test(clues: u8) -> Vec<Event> {
    install_registry();
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.card_code = CardCode::new(ROLAND);
    inv.current_location = Some(loc_id);
    inv.skills.willpower = 3; // base 3
    assert!(
        inv.cards_in_play.is_empty(),
        "the bonus must come from the investigator-card elder-sign bridge (card_code), \
         not a played card — guard against a fixture change pre-populating cards_in_play",
    );

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
    let action = Action::Player(PlayerAction::PerformSkillTest {
        investigator: inv_id,
        skill: SkillKind::Willpower,
        difficulty: 3,
    });
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    let result = drive(state, action, resolver);
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
