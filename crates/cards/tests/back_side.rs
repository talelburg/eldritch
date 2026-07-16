//! Integration: double-sided act/agenda cards carry their reverse side
//! (`back_name`/`back_text`) through the pipeline into the corpus (#558,
//! slice 1). Own process so it can install the process-global registry
//! against the real `cards` corpus.

use game_core::state::CardCode;

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Agenda 01105 ("What's Going On?!") flips to "A Lapse in Time", whose
/// reverse text starts the discard/horror choice. Confirms both back fields
/// survive ingestion.
#[test]
fn agenda_01105_carries_its_reverse() {
    let reg = game_core::card_registry::current().expect("registry installed");
    let m = (reg.metadata_for)(&CardCode::new("01105")).expect("01105 metadata");
    assert_eq!(m.back_name.as_deref(), Some("A Lapse in Time"));
    assert!(
        m.back_text
            .as_deref()
            .unwrap_or_default()
            .contains("discard"),
        "01105 back_text should mention discarding: {:?}",
        m.back_text
    );
}

/// A single-sided player card (Machete 01020, an asset) has no reverse.
/// (Investigator cards like 01001 *do* carry `back_text` — their
/// deckbuilding block — so they aren't the right negative case here.)
#[test]
fn single_sided_card_has_no_reverse() {
    let reg = game_core::card_registry::current().expect("registry installed");
    let m = (reg.metadata_for)(&CardCode::new("01020")).expect("01020 metadata");
    assert_eq!(m.back_name, None);
    assert_eq!(m.back_text, None);
}
