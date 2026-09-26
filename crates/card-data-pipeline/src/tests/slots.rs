use super::*;

#[test]
fn parses_bare_slot() {
    assert_eq!(parse_slots(Some("Hand")), vec!["Hand"]);
    assert_eq!(parse_slots(Some("Accessory")), vec!["Accessory"]);
}

#[test]
fn parses_multi_slot_xn_notation() {
    assert_eq!(parse_slots(Some("Hand x2")), vec!["Hand", "Hand"]);
    assert_eq!(parse_slots(Some("Arcane x2")), vec!["Arcane", "Arcane"]);
}

#[test]
fn drops_unknown_slots() {
    // Head only appears on core_2026 cards we don't ingest.
    assert!(parse_slots(Some("Head")).is_empty());
    assert!(parse_slots(Some("Wibble")).is_empty());
}

#[test]
fn handles_missing_or_empty() {
    assert!(parse_slots(None).is_empty());
    assert!(parse_slots(Some("")).is_empty());
    assert!(parse_slots(Some("   ")).is_empty());
}

#[test]
fn dot_separated_repeats_are_dropped() {
    // The "Hand. Hand." pattern was the bug-discovery shape — the
    // original parser split on '.' and would have emitted two Hand
    // slots from this. Upstream uses "Hand x2" instead. If they
    // ever switch back, this regression test pins the breakage so
    // we notice rather than silently mis-emit.
    assert!(parse_slots(Some("Hand. Hand.")).is_empty());
}

#[test]
fn zero_count_emits_no_slots() {
    // `Foo x0` is degenerate but shouldn't crash; emit nothing.
    assert!(parse_slots(Some("Hand x0")).is_empty());
}

#[test]
fn high_count_does_not_crash() {
    // No real card has this; just guard against panics on weird
    // upstream data.
    assert_eq!(parse_slots(Some("Hand x10")).len(), 10);
}
