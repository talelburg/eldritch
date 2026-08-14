//! End-to-end test that Deduction (01039):
//! 1. Contributes 1 intellect icon when committed to a skill test.
//! 2. Raises the Investigate follow-up's **single** discovery to 2 clues
//!    at the tested location on a successful Investigate — one discovery
//!    of 2, not two of 1 (#471; see the **Discovery** entry in
//!    `CONTEXT.md`).
//! 3. Discovers no clues on a failed Investigate.
//! 4. Does not raise the count on a non-Investigate skill test.
//!
//! Closes the Phase-3 acceptance criterion for #39: the commit-time
//! icon + the discovery-count bonus both work end-to-end with the
//! real card and real registry.

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

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Build a state with Deduction in hand, the active investigator at
/// `LocationId(10)` with `initial_clues` clues there, in the
/// Investigation phase, against a single-`Numeric(0)` chaos bag.
fn state_with_deduction(
    initial_clues: u8,
    shroud: u8,
) -> (game_core::GameState, InvestigatorId, LocationId) {
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
    // shroud 4 → succeed by 0. **One** discovery of 2 clues: the
    // Investigate follow-up's own discovery, its count raised from 1
    // to 2 by the `bonus_clues_discovered` accumulator Deduction
    // populates at commit. Location ends with 0 of 2 clues;
    // controller carries 2.
    //
    // The event *count* is the whole point of this assertion — final
    // clue totals cannot tell one discovery of 2 from two of 1, and
    // that difference is what Cover Up 01007 keys off (#471).
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
    assert_eq!(result.state.locations[&loc].clues, 0);
    assert_eq!(inv.clues, 2);
    assert_event_count!(result.events, 1, Event::CluePlaced { .. });
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 2 } if *investigator == id
    );
}

#[test]
fn deduction_at_a_one_clue_location_discovers_exactly_one() {
    // Deduction "doesn't allow you to discover clues that aren't at that
    // location. If your location has 1 clue at it, you can only discover 1
    // clue at most when you investigate it" (ArkhamDB 01007/01039 FAQ). The
    // combined count of 2 is capped to the location's 1 clue **at emission**,
    // so the single discovery reports the number actually taken.
    let (state, id, loc) = state_with_deduction(1, 4);
    let result = drive_committing_deduction(state);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result.state.locations[&loc].clues, 0);
    assert_eq!(result.state.investigators[&id].clues, 1);
    assert_event_count!(result.events, 1, Event::CluePlaced { .. });
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == id
    );
}

#[test]
fn failed_investigate_discovers_no_clues() {
    // Shroud 99 — even with Deduction's 1 intellect icon, 3 + 0 + 1 = 4
    // << 99 → fail by 95. Deduction's `OnCommit` accumulate is ungated on
    // outcome and does run, but the Investigate follow-up is success-only,
    // so nothing ever reads it and no clue moves.
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
