use crate::engine::TimingEvent;
use crate::state::{Continuation, EmitStep};

#[test]
fn emit_step_walks_the_sequence_with_the_resolve_step_between_when_and_at() {
    let mut step = EmitStep::When;
    let mut walk = vec![step];
    while let Some(next) = step.next() {
        step = next;
        walk.push(step);
    }
    assert_eq!(
        walk,
        vec![
            EmitStep::When,
            EmitStep::ResolveCondition,
            EmitStep::At,
            EmitStep::After
        ],
        "the condition resolves between the `when` and `at` cells (RR Nested Sequences)"
    );
    assert!(
        EmitStep::ResolveCondition.cell().is_none(),
        "the resolve step scans no cell — it resolves the condition"
    );
}

#[test]
fn emit_event_frame_roundtrips_serde() {
    let frame = Continuation::EmitEvent {
        event: TimingEvent::RoundEnded,
        step: EmitStep::ResolveCondition,
    };
    let json = serde_json::to_string(&frame).expect("serialize");
    let back: Continuation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(frame, back);
}
