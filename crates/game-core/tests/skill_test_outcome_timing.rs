//! End-to-end coverage of the general skill-test-outcome timing point
//! (`TimingEvent::SkillTestResolved`): it fires for *every*
//! resolved test, not just a successful Investigate, and opens no window when
//! nothing listens.
//!
//! Own integration-test process (a process holds one `MockRegistry`) so it can
//! build a mock registry without colliding with other `tests/*.rs`. Mirrors
//! `on_skill_test_resolution.rs`.

use std::sync::LazyLock;

use card_dsl::dsl::{
    self, EventPattern, EventTiming, InvestigatorTarget, TestOutcome, TestedLocationScope,
};
use game_core::assert_event;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};

/// Mock threat-area card: a **forced** ability keyed to *any* successful skill
/// test (`kind: None`), dealing 1 horror to the controller. Forced (not a
/// reaction) so it fires automatically in `queue_event`'s forced phase — no
/// reaction-window resolution needed to observe the timing point.
const ANY_SUCCESS_FORCED: &str = "MOCK-STR-ANY-SUCCESS";

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // `TEST_INV` rides `build`'s composed `metadata_for_test_inv`, so capacity
        // reads work under this registry.
        cards: MockRegistry::new()
            .with_abilities(ANY_SUCCESS_FORCED, || {
                vec![dsl::forced_on_event(
                    EventPattern::SkillTestResolved {
                        outcome: TestOutcome::Success,
                        kind: None,
                        by_controller: true,
                        tested_location: TestedLocationScope::Any,
                        margin: None,
                    },
                    EventTiming::After,
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

/// `INV`'s open turn at `HERE`, with a single-`Numeric(0)` chaos bag and the
/// given threat-area card codes in play.
fn session_with_threat_area(threat: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).threat_area(threat.iter().copied()))
        .with_location(test_support::location(HERE))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
}

#[test]
fn general_timing_point_fires_for_non_investigate_test() {
    // A bare plain skill test is kind = Plain (not Investigate). Base intellect
    // 3 + Numeric(0) = 3 >= difficulty 2 -> success. The forced ability keyed to
    // `SkillTestResolved { Success, kind: None }` must fire on this Plain test.
    let session = session_with_threat_area(&[ANY_SUCCESS_FORCED])
        .test_skill(INV, SkillKind::Intellect, 2)
        .commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, .. }
            if *investigator == INV
    );
    assert_eq!(
        session.state().investigators[&INV].horror(),
        1,
        "the general SkillTestResolved timing point must fire the forced ability \
         on a passing Plain (non-Investigate) test",
    );
}

#[test]
fn general_timing_point_opens_no_window_without_a_listener() {
    // A passing Plain test with no listening card resolves straight back to the
    // turn menu and takes no horror — generalizing the emit adds no spurious
    // window/prompt.
    let session = session_with_threat_area(&[])
        .test_skill(INV, SkillKind::Intellect, 2)
        .commit(&[]);

    // A plain test with no listener opens no window: the commit runs on to the
    // turn menu rather than resting at one.
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, .. } if *investigator == INV
    );
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}
