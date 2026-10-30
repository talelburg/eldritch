//! `SkillTestResolved`'s resolve step re-totals the test.
//!
//! Lucky! 01080 prints *"Fast. Play when you would fail a skill test. Get +2 to
//! your skill value for that test."* — a `when`-cell modifier that can turn the
//! verdict the condition announced into the other one. So the resolve step
//! re-reads the total; if the verdict no longer matches, it voids the rest of
//! its own sequence (the `at` and `after` cells of the stale verdict never run)
//! and rewinds the test to ST.5, which announces the fresh verdict in a fresh
//! sequence.
//!
//! Machinery no Core card reaches in *both* directions, so it is driven with mock
//! "+2 / −2" forced abilities in the threat area and cell markers: one
//! `ResourcesGained` whose amount names the cell that fired it.

use std::sync::LazyLock;

use card_dsl::dsl::{
    self, Ability, Effect, EventPattern, EventTiming, InvestigatorTarget, ModifierScope, Stat,
    TestOutcome, TestedLocationScope,
};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

/// When you would fail: +2 to the tested skill.
const BOOST: &str = "MOCK-RD-BOOST";
/// When you would succeed: −2 to the tested skill.
const DRAG: &str = "MOCK-RD-DRAG";
/// Marks the `at` (10) and `after` (11) cells of a failure.
const WATCH_FAIL: &str = "MOCK-RD-WATCH-FAIL";
/// Marks the `after` cell of a success (21).
const WATCH_SUCCESS: &str = "MOCK-RD-WATCH-SUCCESS";
/// Marks the `when` cell of a success (20).
const WHEN_SUCCESS: &str = "MOCK-RD-WHEN-SUCCESS";

const FAIL_AT: u8 = 10;
const FAIL_AFTER: u8 = 11;
const SUCCESS_WHEN: u8 = 20;
const SUCCESS_AFTER: u8 = 21;

fn resolved(outcome: TestOutcome) -> EventPattern {
    EventPattern::SkillTestResolved {
        outcome,
        kind: None,
        by_controller: true,
        tested_location: TestedLocationScope::Any,
        margin: None,
    }
}

fn mark(amount: u8) -> Effect {
    dsl::gain_resources(InvestigatorTarget::You, amount)
}

fn forced(outcome: TestOutcome, timing: EventTiming, effect: Effect) -> Ability {
    dsl::forced_on_event(resolved(outcome), timing, effect)
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(BOOST, || {
                vec![forced(
                    TestOutcome::Failure,
                    EventTiming::When,
                    dsl::modify(Stat::TestedSkill, 2, ModifierScope::ThisSkillTest),
                )]
            })
            .with_abilities(DRAG, || {
                vec![forced(
                    TestOutcome::Success,
                    EventTiming::When,
                    dsl::modify(Stat::TestedSkill, -2, ModifierScope::ThisSkillTest),
                )]
            })
            .with_abilities(WATCH_FAIL, || {
                vec![
                    forced(TestOutcome::Failure, EventTiming::At, mark(FAIL_AT)),
                    forced(TestOutcome::Failure, EventTiming::After, mark(FAIL_AFTER)),
                ]
            })
            .with_abilities(WATCH_SUCCESS, || {
                vec![forced(
                    TestOutcome::Success,
                    EventTiming::After,
                    mark(SUCCESS_AFTER),
                )]
            })
            .with_abilities(WHEN_SUCCESS, || {
                vec![forced(
                    TestOutcome::Success,
                    EventTiming::When,
                    mark(SUCCESS_WHEN),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// `INV`'s open turn at `HERE` (stock 3 in every skill), a single `Numeric(0)`
/// in the bag, and `threat` in the threat area.
fn session(threat: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(INV.0).at(HERE).threat_area(threat.iter().copied()))
        .with_location(test_support::location(HERE))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
}

fn marked(session: &TestSession, amount: u8) -> bool {
    session.events().iter().any(|e| {
        matches!(e, Event::ResourcesGained { investigator, amount: a }
            if *investigator == INV && *a == amount)
    })
}

#[test]
fn a_when_cell_bonus_that_turns_a_failure_into_a_success_voids_the_failure() {
    // Intellect 3 + 0 against 4 fails by 1; the +2 makes it 5, a success by 1.
    let session = session(&[BOOST, WATCH_FAIL, WATCH_SUCCESS, WHEN_SUCCESS])
        .test_skill(INV, SkillKind::Intellect, 4)
        .commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::SkillTestFailed { .. });
    assert_event_count!(session.events(), 1, Event::SkillTestSucceeded { .. });
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 1 }
            if *investigator == INV
    );
    assert!(
        !marked(&session, FAIL_AT) && !marked(&session, FAIL_AFTER),
        "the voided failure's at/after cells must not run: {:?}",
        session.events()
    );
    assert!(
        marked(&session, SUCCESS_WHEN),
        "the fresh success sequence opens its own when cell"
    );
    assert!(marked(&session, SUCCESS_AFTER), "and its after cell");
}

#[test]
fn a_when_cell_penalty_that_turns_a_success_into_a_failure_voids_the_success() {
    // Intellect 3 + 0 against 2 succeeds by 1; the −2 makes it 1, a failure by 1.
    let session = session(&[DRAG, WATCH_FAIL, WATCH_SUCCESS])
        .test_skill(INV, SkillKind::Intellect, 2)
        .commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event_count!(session.events(), 1, Event::SkillTestFailed { .. });
    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == INV
    );
    assert!(
        !marked(&session, SUCCESS_AFTER),
        "the voided success's after cell must not run"
    );
    assert!(
        marked(&session, FAIL_AT) && marked(&session, FAIL_AFTER),
        "the fresh failure sequence runs its at and after cells"
    );
}

#[test]
fn a_bonus_that_leaves_the_verdict_standing_logs_the_retotalled_margin() {
    // Intellect 3 + 0 against 6 fails by 3; +2 makes it fail by 1. The verdict
    // stands, so the sequence carries on, and the one failure logged carries
    // the re-totalled margin.
    let session = session(&[BOOST, WATCH_FAIL])
        .test_skill(INV, SkillKind::Intellect, 6)
        .commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_event_count!(session.events(), 1, Event::SkillTestFailed { .. });
    assert_event!(
        session.events(),
        Event::SkillTestFailed { investigator, by: 1, .. } if *investigator == INV
    );
    assert!(marked(&session, FAIL_AT) && marked(&session, FAIL_AFTER));
}
