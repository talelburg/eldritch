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

/// The two non-numeric printed costs reach the engine as **different** values,
/// and the play path's arm split depends on it: a `"–"` cost is `None`, while
/// an `X` cost is `ArkhamDB`'s `-2` sentinel carried through the pipeline. If a
/// snapshot refresh ever collapsed them, an `X`-cost card would silently become
/// a `"–"` card (unplayable by rule) or vice versa.
///
/// See `check_play_resource_cost_payable` in `game-core`, and
/// `data/official-faq/Frequently_Asked_Questions.md`: *"Cards with a cost of
/// '–' have no cost that can be paid, and therefore cannot be played."*
#[test]
fn a_dash_cost_and_an_x_cost_ingest_differently() {
    let necronomicon = by_code("01009").expect("The Necronomicon (01009) must be in the corpus");
    assert_eq!(necronomicon.name, "The Necronomicon");
    assert_eq!(
        necronomicon.play_cost(),
        None,
        "a printed \"–\" cost must ingest as None"
    );

    let twin_45s = by_code("02010").expect("Jenny's Twin .45s (02010) must be in the corpus");
    assert_eq!(twin_45s.name, "Jenny's Twin .45s");
    assert_eq!(
        twin_45s.play_cost(),
        Some(-2),
        "a printed X cost must ingest as the -2 sentinel, not as None"
    );
}
