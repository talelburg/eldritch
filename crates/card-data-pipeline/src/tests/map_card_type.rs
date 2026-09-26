use super::*;

#[test]
fn map_card_type_maps_all_known_types() {
    for (type_code, expected) in [
        ("investigator", "Investigator"),
        ("asset", "Asset"),
        ("event", "Event"),
        ("skill", "Skill"),
        ("treachery", "Treachery"),
        ("enemy", "Enemy"),
        ("location", "Location"),
        ("agenda", "Agenda"),
        ("act", "Act"),
        ("scenario", "Scenario"),
        ("story", "Story"),
    ] {
        assert_eq!(map_card_type(Some(type_code), "01001").unwrap(), expected);
    }
}

#[test]
fn map_card_type_errors_on_unknown_type() {
    let err = map_card_type(Some("wibble"), "01001").unwrap_err();
    assert!(err.contains("wibble"), "{err}");
    assert!(err.contains("01001"), "{err}");
}

#[test]
fn map_card_type_errors_on_missing_type() {
    let err = map_card_type(None, "01001").unwrap_err();
    assert!(err.contains("missing"), "{err}");
    assert!(err.contains("01001"), "{err}");
}
