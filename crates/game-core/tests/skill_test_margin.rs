//! The `margin` scope on `EventPattern::SkillTestResolved`: an ability
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
    self, EventPattern, EventTiming, InvestigatorTarget, MarginScope, ModifierScope, Stat,
    TestOutcome, TestedLocationScope,
};
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup, TestSession};

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

/// Forced, when you would fail a test: +3 to the tested skill (turns it into a
/// success).
const FLIP_TO_SUCCESS: &str = "MOCK-MARGIN-FLIP-UP";
/// Forced, when you would succeed at a test: -3 to the tested skill (turns it
/// into a failure).
const FLIP_TO_FAILURE: &str = "MOCK-MARGIN-FLIP-DOWN";
/// Forced, when you would fail a test: 1 horror. Its only job is to make the
/// `when` cell's forced abilities a run of two, whose lapse check rewrites the
/// stored result once the flip resolves.
const SPARE_ON_FAILURE: &str = "MOCK-MARGIN-SPARE-FAILURE";
/// Forced, when you would succeed at a test: 1 horror. As `SPARE_ON_FAILURE`.
const SPARE_ON_SUCCESS: &str = "MOCK-MARGIN-SPARE-SUCCESS";
/// Reaction, when you would fail a test by 2 or less: 1 horror.
const REACT_FAIL_BY_TWO_OR_LESS: &str = "MOCK-MARGIN-REACT-FAILURE-LE2";
/// Reaction, when you would succeed at a test by 1 or more: 1 horror.
const REACT_SUCCEED_BY_ONE_OR_MORE: &str = "MOCK-MARGIN-REACT-SUCCESS-GE1";
/// Reaction, when you would fail a test (no margin): 1 horror.
const REACT_ON_FAILURE: &str = "MOCK-MARGIN-REACT-FAILURE";

fn would(outcome: TestOutcome, margin: Option<MarginScope>) -> EventPattern {
    EventPattern::SkillTestResolved {
        outcome,
        kind: None,
        by_controller: true,
        tested_location: TestedLocationScope::Any,
        margin,
    }
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
            .with_abilities(FLIP_TO_SUCCESS, || {
                vec![dsl::forced_on_event(
                    would(TestOutcome::Failure, None),
                    EventTiming::When,
                    dsl::modify(Stat::TestedSkill, 3, ModifierScope::ThisSkillTest),
                )]
            })
            .with_abilities(FLIP_TO_FAILURE, || {
                vec![dsl::forced_on_event(
                    would(TestOutcome::Success, None),
                    EventTiming::When,
                    dsl::modify(Stat::TestedSkill, -3, ModifierScope::ThisSkillTest),
                )]
            })
            .with_abilities(SPARE_ON_FAILURE, || {
                vec![dsl::forced_on_event(
                    would(TestOutcome::Failure, None),
                    EventTiming::When,
                    dsl::deal_horror(InvestigatorTarget::You, 1u8),
                )]
            })
            .with_abilities(SPARE_ON_SUCCESS, || {
                vec![dsl::forced_on_event(
                    would(TestOutcome::Success, None),
                    EventTiming::When,
                    dsl::deal_horror(InvestigatorTarget::You, 1u8),
                )]
            })
            .with_abilities(REACT_FAIL_BY_TWO_OR_LESS, || {
                vec![dsl::reaction_on_event(
                    would(TestOutcome::Failure, Some(MarginScope::at_most(2))),
                    EventTiming::When,
                    dsl::deal_horror(InvestigatorTarget::You, 1u8),
                )]
            })
            .with_abilities(REACT_SUCCEED_BY_ONE_OR_MORE, || {
                vec![dsl::reaction_on_event(
                    would(TestOutcome::Success, Some(MarginScope::at_least(1))),
                    EventTiming::When,
                    dsl::deal_horror(InvestigatorTarget::You, 1u8),
                )]
            })
            .with_abilities(REACT_ON_FAILURE, || {
                vec![dsl::reaction_on_event(
                    would(TestOutcome::Failure, None),
                    EventTiming::When,
                    dsl::deal_horror(InvestigatorTarget::You, 1u8),
                )]
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

/// Runs a test at `difficulty` with `flip`, `spare` and `reaction` in `INV`'s
/// threat area, ordering the `when` cell's two forced abilities flip-first.
/// Once the flip resolves, the run's lapse check rewrites the stored result in
/// the other direction (and withdraws `spare`), so the cell's reaction scan
/// meets an announced outcome and a stored result that disagree.
fn session_after_flip(flip: &str, spare: &str, reaction: &str, difficulty: i8) -> TestSession {
    let session: TestSession = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).threat_area([flip, spare, reaction]))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Intellect, difficulty)
        .commit(&[]);
    let flip_card = session.state().card(INV, flip);
    session.pick(flip_card)
}

#[test]
fn a_failure_margin_does_not_read_a_stored_success() {
    // 3 against 5 fails by 2; the +3 makes the stored result a success by 1,
    // which "fail by 2 or less" must not read as its margin.
    let session = session_after_flip(
        FLIP_TO_SUCCESS,
        SPARE_ON_FAILURE,
        REACT_FAIL_BY_TWO_OR_LESS,
        5,
    );
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}

#[test]
fn a_success_margin_does_not_read_a_stored_failure() {
    // 3 against 1 succeeds by 2; the -3 makes the stored result a failure by
    // 1, which "succeed by 1 or more" must not read as its margin.
    let session = session_after_flip(
        FLIP_TO_FAILURE,
        SPARE_ON_SUCCESS,
        REACT_SUCCEED_BY_ONE_OR_MORE,
        1,
    );
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}

#[test]
fn a_flipped_verdict_withholds_the_stale_verdicts_reaction() {
    // 3 against 5 fails; the +3 makes it a success by 1 before the `when`
    // cell's reaction scan runs. "When you would fail" no longer holds, so the
    // scan never offers it, rather than offering it against the announced
    // failure for the window's lapse check to withdraw.
    let session = session_after_flip(FLIP_TO_SUCCESS, SPARE_ON_FAILURE, REACT_ON_FAILURE, 5);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}
