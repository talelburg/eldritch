use super::*;

#[test]
fn process_raw_skips_skeleton_entries_silently() {
    // Skeleton entries (no `name`) are reserved-code placeholders
    // upstream; the pipeline should ignore them rather than error.
    let mut raw = raw_card("01999");
    raw.name = None;
    let mut all = BTreeMap::new();
    process_raw(raw, &mut all, Path::new("fixture.json"))
        .expect("skeleton entries are not an error");
    assert!(all.is_empty(), "skipped entry should not be inserted");
}

#[test]
fn process_raw_rejects_duplicate_code() {
    let mut all = BTreeMap::new();
    process_raw(raw_card("01001"), &mut all, Path::new("fixture.json"))
        .expect("first insert succeeds");
    let err = process_raw(raw_card("01001"), &mut all, Path::new("fixture.json"))
        .expect_err("second insert with same code errors");
    assert!(err.contains("duplicate"), "{err}");
    assert!(err.contains("01001"), "{err}");
}

#[test]
fn process_raw_inserts_normalized_card() {
    let mut all = BTreeMap::new();
    process_raw(raw_card("01001"), &mut all, Path::new("fixture.json")).unwrap();
    assert_eq!(all.len(), 1);
    assert!(all.contains_key("01001"));
}

#[test]
fn process_raw_wraps_normalize_error_with_path() {
    let mut raw = raw_card("01001");
    raw.faction_code = Some("wibble".to_owned());
    let mut all = BTreeMap::new();
    let err = process_raw(raw, &mut all, Path::new("fixture.json")).unwrap_err();
    // The wrapping format is "normalizing card in <path>: <inner>"
    assert!(err.contains("fixture.json"), "{err}");
    assert!(err.contains("wibble"), "{err}");
}

#[test]
fn scenario_type_card_is_skipped() {
    let mut raw = raw_card("01104");
    raw.type_code = Some("scenario".to_owned());
    let mut all = BTreeMap::new();
    process_raw(raw, &mut all, Path::new("fixture.json")).expect("scenario skip is not an error");
    assert!(all.is_empty(), "scenario-type cards are skipped");
}
