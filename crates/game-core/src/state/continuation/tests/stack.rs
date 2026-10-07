//! The continuation stack by itself, without `apply` (#925 Testing Decisions
//! seam 3): what it answers and what it refuses.

use super::profile::every_variant_rows;
use super::*;
use crate::state::UpkeepPhaseFrame;
use crate::test_support;

// --- Wire format -----------------------------------------------------------

/// Every frame variant (and value-level split), serialised by the engine as it
/// stood before the stack became an owned type (#928). Captured once and never
/// regenerated: a change to it is a wire-format change.
const WIRE_FIXTURE: &str = include_str!("fixtures/stack_wire.json");

/// The stack reads today's wire format into exactly the frames that produced
/// it, and writes them back unchanged: a plain array of externally tagged
/// frames, as the `web` client and the server's persisted seed state expect.
#[test]
fn the_stack_round_trips_the_pre_928_wire_format() {
    let stack: ContinuationStack =
        serde_json::from_str(WIRE_FIXTURE).expect("fixture deserializes");
    let expected: Vec<Continuation> = every_variant_rows().into_iter().map(|(f, _)| f).collect();
    assert!(
        stack.iter().eq(expected.iter()),
        "fixture frames differ from the every-variant table"
    );
    let fixture: serde_json::Value = serde_json::from_str(WIRE_FIXTURE).expect("fixture is JSON");
    assert_eq!(serde_json::to_value(&stack).expect("serialize"), fixture);
}

// --- Fixtures ---------------------------------------------------------------

fn skill_test(id: u32) -> InFlightSkillTest {
    test_support::test_skill_test(
        SkillTestId(id),
        InvestigatorId(1),
        SkillKind::Willpower,
        SkillTestKind::Plain,
        3,
    )
}

fn anchor() -> Continuation {
    Continuation::UpkeepPhase(UpkeepPhaseFrame {
        resume: UpkeepResume::Begins,
    })
}

fn effect() -> EffectFrame {
    EffectFrame::Leaf {
        effect: Box::new(Effect::Seq(vec![])),
        ctx: EvalContext::for_controller(InvestigatorId(1)),
    }
}

fn hand_size_discard() -> HandSizeDiscard {
    HandSizeDiscard {
        remaining: vec![InvestigatorId(1)],
    }
}

fn mulligan() -> Continuation {
    Continuation::Mulligan {
        remaining: vec![InvestigatorId(1)],
    }
}

// --- Push-time checks -------------------------------------------------------

#[test]
fn a_pushed_payload_becomes_the_top_frame() {
    let mut stack = ContinuationStack::new();
    stack.push(hand_size_discard());
    assert_eq!(
        stack.top(),
        Some(&Continuation::HandSizeDiscard(hand_size_discard()))
    );
}

#[test]
fn an_anchor_may_be_pushed_over_frames_that_are_not_queued_abilities() {
    let mut stack = ContinuationStack::new();
    stack.push(mulligan());
    stack.push(anchor());
    assert_eq!(stack.top(), Some(&anchor()));
}

/// ADR 0003: the Ghoul-movement bug class, caught at the push that buries the
/// ability rather than phases later.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "would bury a queued ability frame")]
fn pushing_an_anchor_over_a_queued_ability_is_refused() {
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.push(effect());
    stack.push(anchor());
}

/// The check covers the whole stack, not only the frame directly beneath.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "would bury a queued ability frame")]
fn pushing_an_anchor_over_a_deeply_buried_queued_ability_is_refused() {
    let mut stack = ContinuationStack::new();
    stack.push(effect());
    stack.push(hand_size_discard());
    stack.push(anchor());
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a second skill test")]
fn pushing_a_second_skill_test_is_refused() {
    let mut stack = ContinuationStack::new();
    stack.push(skill_test(0));
    stack.push(effect());
    stack.push(skill_test(1));
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "only `insert_ending_at_bottom` may create one")]
fn pushing_an_ending_frame_is_refused() {
    let mut stack = ContinuationStack::new();
    stack.push(Continuation::ScenarioEnd {
        step: ScenarioEndStep::EmitGameEnd,
    });
}

// --- The bottom insertion ---------------------------------------------------

#[test]
fn the_ending_frame_is_inserted_beneath_everything_under_way() {
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.push(skill_test(0));
    stack.insert_ending_at_bottom();
    assert_eq!(
        stack,
        vec![
            Continuation::ScenarioEnd {
                step: ScenarioEndStep::EmitGameEnd,
            },
            anchor(),
            Continuation::SkillTest(skill_test(0)),
        ]
    );
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a second ending frame")]
fn a_second_ending_frame_is_refused() {
    let mut stack = ContinuationStack::new();
    stack.insert_ending_at_bottom();
    stack.insert_ending_at_bottom();
}

// --- Typed accessors --------------------------------------------------------

#[test]
fn top_mut_edits_the_top_frame_in_place() {
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.push(hand_size_discard());
    stack.top_mut::<HandSizeDiscard>().remaining.clear();
    assert_eq!(
        stack.top(),
        Some(&Continuation::HandSizeDiscard(HandSizeDiscard {
            remaining: vec![]
        }))
    );
}

#[test]
#[should_panic(expected = "expected a SkillTest frame on top, found HandSizeDiscard")]
fn top_mut_of_the_wrong_kind_panics() {
    let mut stack = ContinuationStack::new();
    stack.push(hand_size_discard());
    stack.top_mut::<InFlightSkillTest>();
}

#[test]
#[should_panic(expected = "expected a SkillTest frame on top, found an empty stack")]
fn top_mut_of_an_empty_stack_panics() {
    ContinuationStack::new().top_mut::<InFlightSkillTest>();
}

#[test]
fn pop_expect_returns_the_top_payload_and_exposes_the_frame_beneath() {
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.push(skill_test(0));
    assert_eq!(stack.pop_expect::<InFlightSkillTest>(), skill_test(0));
    assert_eq!(stack.top(), Some(&anchor()));
}

#[test]
#[should_panic(expected = "expected a Effect frame on top, found UpkeepPhase")]
fn pop_expect_of_the_wrong_kind_panics() {
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.pop_expect::<EffectFrame>();
}

#[test]
fn topmost_of_finds_a_buried_frame_of_the_named_kind() {
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.push(skill_test(0));
    stack.push(effect());
    assert_eq!(
        stack.topmost_of::<InFlightSkillTest>(),
        Some(&skill_test(0))
    );
    assert_eq!(stack.topmost_of::<HandSizeDiscard>(), None);
}

#[test]
fn remove_topmost_takes_a_buried_frame_and_keeps_the_rest_in_order() {
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.push(skill_test(0));
    stack.push(effect());
    assert_eq!(
        stack.remove_topmost::<InFlightSkillTest>(),
        Some(skill_test(0))
    );
    assert_eq!(stack, vec![anchor(), Continuation::Effect(effect())]);
    assert_eq!(stack.remove_topmost::<InFlightSkillTest>(), None);
}

// --- At rest ----------------------------------------------------------------

#[test]
fn the_stack_is_at_rest_empty_or_with_a_prompt_on_top() {
    assert!(ContinuationStack::new().is_at_rest());
    let mut stack = ContinuationStack::new();
    stack.push(anchor());
    stack.push(hand_size_discard());
    assert!(stack.is_at_rest());
}

#[test]
fn the_stack_is_not_at_rest_with_a_driven_or_inert_frame_on_top() {
    let mut driven = ContinuationStack::new();
    driven.push(Continuation::PlayerDraw {
        investigator: InvestigatorId(1),
        chain_count: 0,
        surge_pending: false,
    });
    assert!(!driven.is_at_rest());
    let mut inert = ContinuationStack::new();
    inert.push(anchor());
    assert!(!inert.is_at_rest());
}

// --- The unchecked constructor ----------------------------------------------

#[test]
fn the_unchecked_constructor_builds_stacks_the_checks_would_refuse() {
    let frames = vec![
        Continuation::Effect(effect()),
        anchor(),
        Continuation::SkillTest(skill_test(0)),
        Continuation::SkillTest(skill_test(1)),
        Continuation::ScenarioEnd {
            step: ScenarioEndStep::EmitGameEnd,
        },
    ];
    let stack = test_support::from_frames_unchecked(frames.clone());
    assert_eq!(stack, frames);
}
