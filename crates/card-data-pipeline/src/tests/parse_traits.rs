use super::*;

#[test]
fn parse_traits_handles_missing_and_empty() {
    assert!(parse_traits(None).is_empty());
    assert!(parse_traits(Some("")).is_empty());
    assert!(parse_traits(Some("   ")).is_empty());
}

#[test]
fn parse_traits_single_trait() {
    assert_eq!(parse_traits(Some("Detective.")), vec!["Detective"]);
}

#[test]
fn parse_traits_multi_trait_dot_separated() {
    // ArkhamDB convention: trailing dot on each trait.
    assert_eq!(
        parse_traits(Some("Item. Tool. Relic.")),
        vec!["Item", "Tool", "Relic"]
    );
}

#[test]
fn parse_traits_trims_whitespace_and_skips_empties() {
    // The split on '.' produces an empty trailing element after
    // the final dot; the filter drops it. Also drop interior
    // whitespace-only fragments. Not a known real upstream shape,
    // just defensive against malformed data.
    assert_eq!(parse_traits(Some("Item.  . Tool.")), vec!["Item", "Tool"]);
}
