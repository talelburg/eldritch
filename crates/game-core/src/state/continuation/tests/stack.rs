//! The continuation stack by itself, without `apply` (#925 Testing Decisions
//! seam 3): what it answers and what it refuses.

use super::profile::every_variant_rows;
use super::*;

// --- Wire format -----------------------------------------------------------

/// Every frame variant (and value-level split), serialised by the engine as it
/// stood before the stack became an owned type (#928). Captured once and never
/// regenerated: a change to it is a wire-format change.
const WIRE_FIXTURE: &str = include_str!("fixtures/stack_wire.json");

type Stack = Vec<Continuation>;

/// The stack reads today's wire format into exactly the frames that produced
/// it, and writes them back byte-for-byte in shape: a plain array of
/// externally tagged frames, as the `web` client and the server's persisted
/// seed state expect.
#[test]
fn the_stack_round_trips_the_pre_928_wire_format() {
    let stack: Stack = serde_json::from_str(WIRE_FIXTURE).expect("fixture deserializes");
    let expected: Vec<Continuation> = every_variant_rows().into_iter().map(|(f, _)| f).collect();
    assert!(
        stack.iter().eq(expected.iter()),
        "fixture frames differ from the every-variant table"
    );
    let fixture: serde_json::Value = serde_json::from_str(WIRE_FIXTURE).expect("fixture is JSON");
    assert_eq!(serde_json::to_value(&stack).expect("serialize"), fixture);
}
