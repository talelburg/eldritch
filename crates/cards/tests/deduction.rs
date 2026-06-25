//! End-to-end test that Deduction (01039):
//! 1. Contributes 1 intellect icon when committed to a skill test.
//! 2. Discovers 1 additional clue at the tested location on a
//!    successful Investigate.
//! 3. Does not fire its bonus on a failed Investigate.
//! 4. Does not fire its bonus on a non-Investigate skill test.
//!
//! Closes the Phase-3 acceptance criterion for #39: the commit-time
//! icon + the resolution-time bonus both work end-to-end with the
//! real card and real registry.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{
    drive_skill_test, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
    TestSession,
};
use game_core::{assert_event, assert_event_count, assert_no_event, TurnAction};

const DEDUCTION: &str = "01039";

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Build a state with Deduction in hand, the active investigator at
/// `LocationId(10)` with `initial_clues` clues there, in the
/// Investigation phase, against a single-`Numeric(0)` chaos bag.
fn state_with_deduction(
    initial_clues: u8,
    shroud: u8,
) -> (game_core::GameState, InvestigatorId, LocationId) {
    install_real_registry();
    let id = InvestigatorId(1);
    let loc = LocationId(10);
    let mut inv = test_investigator(1);
    inv.current_location = Some(loc);
    inv.hand = vec![CardCode::new(DEDUCTION)];
    let mut location = test_location(10, "Study");
    location.shroud = shroud;
    location.clues = initial_clues;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, loc)
}

fn drive_committing_deduction(state: game_core::GameState) -> game_core::ApplyResult {
    TestSession::new(state)
        .take(&TurnAction::Investigate {
            investigator: InvestigatorId(1),
        })
        .resolve_choices(|c| {
            c.commit_cards(&[CardCode::new(DEDUCTION)]);
        })
        .run()
}

#[test]
fn investigate_with_committed_deduction_succeeds_at_shroud_4_via_intellect_icon() {
    // 3 intellect + 0 (token) + 1 (Deduction's intellect icon) = 4 vs
    // shroud 4 → succeed by 0. Two CluePlaced events fire: one from
    // the Investigate action's standard `SkillTestFollowUp` (1 clue
    // at the controller's location), and one from Deduction's
    // OnSkillTestResolution bonus (1 clue at the tested location,
    // same location here). Location ends with 0 of 2 clues;
    // controller carries 2.
    let (state, id, loc) = state_with_deduction(2, 4);
    let result = drive_committing_deduction(state);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    // Deduction discards after the test.
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.discard, vec![CardCode::new(DEDUCTION)]);
    assert!(inv.hand.is_empty());
    // Two clues moved: one from the action's standard follow-up, one
    // from Deduction's bonus. Both at the tested location.
    assert_eq!(result.state.locations[&loc].clues, 0);
    assert_eq!(inv.clues, 2);
    assert_event_count!(result.events, 2, Event::CluePlaced { .. });
}

#[test]
fn failed_investigate_does_not_fire_deductions_bonus() {
    // Shroud 99 — even with Deduction's 1 intellect icon, 3 + 0 + 1 = 4
    // << 99 → fail by 95. No bonus clue should fire.
    let (state, id, loc) = state_with_deduction(2, 99);
    let result = drive_committing_deduction(state);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 95, .. }
            if *investigator == id
    );
    // Deduction still discards on a failed test (committed cards
    // discard regardless of outcome).
    assert_eq!(
        result.state.investigators[&id].discard,
        vec![CardCode::new(DEDUCTION)],
    );
    // Location's clues unchanged, controller has none.
    assert_eq!(result.state.locations[&loc].clues, 2);
    assert_eq!(result.state.investigators[&id].clues, 0);
    assert_no_event!(result.events, Event::CluePlaced { .. });
}

#[test]
fn non_investigate_test_does_not_fire_deductions_bonus() {
    // A bare plain skill test is `SkillTestKind::Plain`. Deduction's
    // bonus is gated to Investigate, so even though the test succeeds
    // with Deduction's icon contributing to the total, the bonus must
    // not fire.
    //
    // 3 + 0 + 1 = 4 vs difficulty 4 → succeed by 0. Location keeps
    // its clue (no action follow-up either — a bare plain test's
    // follow-up is `None`).
    let (state, id, loc) = state_with_deduction(1, 4);
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[CardCode::new(DEDUCTION)]);
    let result = drive_skill_test(state, id, SkillKind::Intellect, 4, resolver);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    assert_eq!(result.state.locations[&loc].clues, 1);
    assert_eq!(result.state.investigators[&id].clues, 0);
    assert_no_event!(result.events, Event::CluePlaced { .. });
}

#[test]
fn uncommitted_deduction_does_not_fire_its_bonus() {
    // Deduction in hand but not committed → no icon contribution, no
    // bonus. 3 + 0 < 4 → fail by 1, hand unchanged.
    let (state, id, loc) = state_with_deduction(1, 4);
    let result = TestSession::new(state)
        .take(&TurnAction::Investigate { investigator: id })
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .run();

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == id
    );
    assert_eq!(
        result.state.investigators[&id].hand,
        vec![CardCode::new(DEDUCTION)],
        "uncommitted card stays in hand",
    );
    assert_eq!(result.state.locations[&loc].clues, 1);
    assert_eq!(result.state.investigators[&id].clues, 0);
}
