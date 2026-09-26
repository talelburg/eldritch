use super::*;

fn make_treachery(weakness: bool) -> CardMetadata {
    CardMetadata {
        code: "_test".into(),
        name: "Test".into(),
        text: None,
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_test".into(),
        weakness,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    }
}

#[test]
fn is_weakness_true_when_field_is_true() {
    assert!(make_treachery(true).is_weakness());
}

#[test]
fn is_weakness_false_when_field_is_false() {
    assert!(!make_treachery(false).is_weakness());
}
