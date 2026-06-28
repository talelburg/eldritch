//! End-to-end smoke tests for weakness metadata on the real corpus.
//!
//! Verifies that the pipeline correctly propagated `weakness: true/false`
//! from the pinned snapshot into the generated corpus, and that the
//! `is_weakness()` accessor round-trips through the registry.

use cards::by_code;

#[test]
fn cover_up_is_a_weakness() {
    // Cover Up (01007) is Roland Banks' signature weakness — `subtype_code:
    // "weakness"` in the ArkhamDB snapshot.
    let meta = by_code("01007").expect("Cover Up (01007) must be in the corpus");
    assert_eq!(meta.name, "Cover Up");
    assert!(
        meta.is_weakness(),
        "Cover Up should be marked weakness=true"
    );
}

#[test]
fn holy_rosary_is_not_a_weakness() {
    // Holy Rosary (01059) is a regular Mystic asset — no `subtype_code`.
    let meta = by_code("01059").expect("Holy Rosary (01059) must be in the corpus");
    assert_eq!(meta.name, "Holy Rosary");
    assert!(
        !meta.is_weakness(),
        "Holy Rosary should be marked weakness=false"
    );
}
