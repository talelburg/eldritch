use super::*;

#[test]
fn map_class_maps_all_known_factions() {
    for (faction, expected) in [
        ("guardian", "Guardian"),
        ("seeker", "Seeker"),
        ("rogue", "Rogue"),
        ("mystic", "Mystic"),
        ("survivor", "Survivor"),
        ("neutral", "Neutral"),
        ("mythos", "Mythos"),
    ] {
        assert_eq!(map_class(Some(faction), "01001").unwrap(), expected);
    }
}

#[test]
fn map_class_errors_on_unknown_faction() {
    let err = map_class(Some("wibble"), "01001").unwrap_err();
    assert!(err.contains("wibble"), "{err}");
    assert!(err.contains("01001"), "{err}");
}

#[test]
fn map_class_errors_on_missing_faction() {
    let err = map_class(None, "01001").unwrap_err();
    assert!(err.contains("missing"), "{err}");
    assert!(err.contains("01001"), "{err}");
}
