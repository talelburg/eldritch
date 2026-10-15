//! The `margin` scope on `EventPattern::SkillTestResolved` (#1103): an ability
//! listening for *"by 2 or more"* on a success, or *"by 2 or less"* on a
//! failure, hears only the tests whose margin, measured in the outcome's
//! direction, is inside its bound.
//!
//! Own integration-test process (a process holds one `MockRegistry`), like
//! `skill_test_outcome_timing.rs`. The listeners are **forced** abilities so
//! they fire without a reaction window, and each deals 1 horror: the horror
//! count is the observation.

use std::sync::LazyLock;

use card_dsl::dsl::{
    self, EventPattern, EventTiming, InvestigatorTarget, MarginScope, TestOutcome,
    TestedLocationScope,
};
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};

/// Forced, after any of your tests succeeds by 2 or more: 1 horror.
const SUCCEED_BY_TWO_OR_MORE: &str = "MOCK-MARGIN-SUCCESS-GE2";
/// Forced, after any of your tests fails by 2 or less: 1 horror.
const FAIL_BY_TWO_OR_LESS: &str = "MOCK-MARGIN-FAILURE-LE2";

fn listener(outcome: TestOutcome, margin: MarginScope) -> Vec<dsl::Ability> {
    vec![dsl::forced_on_event(
        EventPattern::SkillTestResolved {
            outcome,
            kind: None,
            by_controller: true,
            tested_location: TestedLocationScope::Any,
            margin: Some(margin),
        },
        EventTiming::After,
        dsl::deal_horror(InvestigatorTarget::You, 1u8),
    )]
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(SUCCEED_BY_TWO_OR_MORE, || {
                listener(TestOutcome::Success, MarginScope::at_least(2))
            })
            .with_abilities(FAIL_BY_TWO_OR_LESS, || {
                listener(TestOutcome::Failure, MarginScope::at_most(2))
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The horror `INV` has after a plain intellect test at `difficulty`, with
/// `listener` in their threat area. `TEST_INV`'s intellect is 3 and the bag
/// holds a single `Numeric(0)`, so the total is always 3.
fn horror_after_test(listener: &str, difficulty: i8) -> u8 {
    let session: TestSession = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).threat_area([listener]))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Intellect, difficulty)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session.state().investigators[&INV].horror()
}

#[test]
fn succeeding_by_exactly_two_matches_at_least_two() {
    // 3 against 1: succeeded by 2.
    assert_eq!(horror_after_test(SUCCEED_BY_TWO_OR_MORE, 1), 1);
}

#[test]
fn succeeding_by_more_than_two_matches_at_least_two() {
    // 3 against 0: succeeded by 3.
    assert_eq!(horror_after_test(SUCCEED_BY_TWO_OR_MORE, 0), 1);
}

#[test]
fn succeeding_by_one_misses_at_least_two() {
    // 3 against 2: succeeded by 1.
    assert_eq!(horror_after_test(SUCCEED_BY_TWO_OR_MORE, 2), 0);
}

#[test]
fn failing_by_exactly_two_matches_at_most_two() {
    // 3 against 5: failed by 2.
    assert_eq!(horror_after_test(FAIL_BY_TWO_OR_LESS, 5), 1);
}

#[test]
fn failing_by_one_matches_at_most_two() {
    // 3 against 4: failed by 1.
    assert_eq!(horror_after_test(FAIL_BY_TWO_OR_LESS, 4), 1);
}

#[test]
fn failing_by_three_misses_at_most_two() {
    // 3 against 6: failed by 3.
    assert_eq!(horror_after_test(FAIL_BY_TWO_OR_LESS, 6), 0);
}

#[test]
fn a_margin_bound_on_failure_never_hears_a_success() {
    // 3 against 3: succeeded by 0, which "by 2 or less" would admit on the
    // wrong outcome.
    assert_eq!(horror_after_test(FAIL_BY_TWO_OR_LESS, 3), 0);
}
