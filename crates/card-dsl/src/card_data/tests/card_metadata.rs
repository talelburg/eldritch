use super::*;
use crate::dsl::CardFilter;

#[test]
fn metadata_serde_roundtrip_preserves_is_fast() {
    let original = CardMetadata {
        code: "01030".into(),
        name: "Magnifying Glass".into(),
        text: Some("Fast.\nYou get +1 [intellect] while investigating.".into()),
        traits: vec!["Item".into(), "Tool".into()],
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Seeker,
            cost: Some(1),
            xp: Some(0),
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
    assert!(matches!(back.kind, CardKind::Asset { is_fast: true, .. }));
}

#[test]
fn play_only_during_turn_reads_the_stored_flag() {
    let event = |play_only_during_turn| CardMetadata {
        code: "01036".into(),
        name: "Mind over Matter".into(),
        traits: vec!["Insight".into()],
        text: Some("Fast. Play only during your turn. …".into()),
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Seeker,
            cost: Some(1),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            play_only_during_turn,
        },
    };
    assert!(event(true).play_only_during_turn());
    assert!(!event(false).play_only_during_turn());
}

#[test]
fn play_cost_reads_asset_and_event_cost_and_none_elsewhere() {
    let asset = |cost| CardMetadata {
        code: "01017".into(),
        name: "Physical Training".into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Guardian,
            cost,
            xp: None,
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    };
    // Fixed cost reads through; X-cost (`None`) reads through as `None`.
    assert_eq!(asset(Some(2)).play_cost(), Some(2));
    assert_eq!(asset(None).play_cost(), None);

    let event = CardMetadata {
        code: "01088".into(),
        name: "Emergency Cache".into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 3,
            play_only_during_turn: false,
        },
    };
    assert_eq!(event.play_cost(), Some(0));

    // A non-playable card type has no play cost.
    let skill = CardMetadata {
        code: "01089".into(),
        name: "Guts".into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Skill {
            class: Class::Survivor,
            xp: None,
            skill_icons: SkillIcons::default(),
            deck_limit: 2,
            commit_limit: None,
        },
    };
    assert_eq!(skill.play_cost(), None);
}

#[test]
fn asset_uses_round_trips() {
    let uses = Some(Uses {
        kind: UseKind::Ammo,
        count: 4,
        discard_when_empty: false,
    });
    let json = serde_json::to_string(&uses).expect("serialize");
    let back: Option<Uses> = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, uses);
}

#[test]
fn card_metadata_serde_roundtrip_preserves_surge_and_peril() {
    let original = CardMetadata {
        code: "_synth_surge_treachery".into(),
        name: "Synth Surge Treachery".into(),
        text: None,
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_synth".into(),
        weakness: false,
        kind: CardKind::Treachery {
            surge: true,
            peril: false,
            quantity: 1,
        },
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
    assert!(matches!(
        back.kind,
        CardKind::Treachery {
            surge: true,
            peril: false,
            ..
        }
    ));
}

#[test]
fn card_type_is_derived_from_kind() {
    let m = CardMetadata {
        code: "x".into(),
        name: "X".into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Skill {
            class: Class::Seeker,
            xp: None,
            skill_icons: SkillIcons::default(),
            deck_limit: 2,
            commit_limit: None,
        },
    };
    assert_eq!(m.card_type(), CardType::Skill);
    assert_eq!(m.class(), Some(Class::Seeker));
}

#[test]
fn encounter_cards_have_no_class() {
    let m = CardMetadata {
        code: "y".into(),
        name: "Y".into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
    };
    assert_eq!(m.card_type(), CardType::Treachery);
    assert_eq!(m.class(), None);
}

#[test]
fn new_encounter_kinds_have_no_class_and_right_type() {
    let loc = CardMetadata {
        code: "01111".into(),
        name: "Study".into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Location {
            shroud: 2,
            printed_clues: ClueValue::PerInvestigator(2),
            victory: None,
        },
    };
    assert_eq!(loc.card_type(), CardType::Location);
    assert_eq!(loc.class(), None);

    let agenda = CardMetadata {
        code: "01105".into(),
        name: "Agenda".into(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Agenda { doom_threshold: 3 },
    };
    assert_eq!(agenda.card_type(), CardType::Agenda);
}

#[test]
fn slots_reads_asset_slots_and_empty_elsewhere() {
    let two_handed = CardMetadata {
        code: "x".into(),
        name: "X".into(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Guardian,
            cost: Some(5),
            xp: Some(4),
            slots: vec![Slot::Hand, Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    };
    assert_eq!(two_handed.slots(), &[Slot::Hand, Slot::Hand]);

    let event = CardMetadata {
        code: "y".into(),
        name: "Y".into(),
        text: None,
        traits: vec![],
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Event {
            class: Class::Seeker,
            cost: Some(1),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    };
    assert!(event.slots().is_empty());
}

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

// ---- printed traits and card filters ----------------------------

/// Old Book of Lore 01031 as printed: an `Item. Tome.` asset.
fn old_book_of_lore() -> CardMetadata {
    CardMetadata {
        code: "01031".into(),
        name: "Old Book of Lore".into(),
        traits: vec!["Item".into(), "Tome".into()],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".into(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Seeker,
            cost: Some(3),
            xp: Some(0),
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    }
}

/// Research Librarian 01032 as printed: an `Ally. Miskatonic.` asset,
/// so an asset with traits, none of them Tome.
fn research_librarian() -> CardMetadata {
    CardMetadata {
        code: "01032".into(),
        name: "Research Librarian".into(),
        traits: vec!["Ally".into(), "Miskatonic".into()],
        kind: CardKind::Asset {
            class: Class::Seeker,
            cost: Some(2),
            xp: Some(0),
            slots: vec![Slot::Ally],
            health: Some(1),
            sanity: Some(1),
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
        ..old_book_of_lore()
    }
}

/// Mind over Matter 01036 as printed: an `Insight.` event.
fn mind_over_matter() -> CardMetadata {
    CardMetadata {
        code: "01036".into(),
        name: "Mind over Matter".into(),
        traits: vec!["Insight".into()],
        kind: CardKind::Event {
            class: Class::Seeker,
            cost: Some(1),
            xp: Some(0),
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 2,
            play_only_during_turn: true,
        },
        ..old_book_of_lore()
    }
}

#[test]
fn has_trait_is_true_for_a_printed_trait() {
    assert!(old_book_of_lore().has_trait("Tome"));
    assert!(old_book_of_lore().has_trait("Item"));
}

#[test]
fn has_trait_is_false_for_an_absent_trait() {
    assert!(!research_librarian().has_trait("Tome"));
}

#[test]
fn has_trait_is_false_for_a_near_miss_spelling() {
    let book = old_book_of_lore();
    assert!(!book.has_trait("Tomes"));
    assert!(!book.has_trait("tome"));
    assert!(!book.has_trait("Tome."));
}

#[test]
fn trait_only_filter_matches_on_the_trait_alone() {
    let tome = CardFilter::with_trait("Tome");
    assert!(tome.matches(&old_book_of_lore()));
    assert!(!tome.matches(&research_librarian()));
}

#[test]
fn kind_only_filter_matches_on_the_card_type_alone() {
    let asset = CardFilter::of_kind(CardType::Asset);
    assert!(asset.matches(&old_book_of_lore()));
    assert!(asset.matches(&research_librarian()));
    assert!(!asset.matches(&mind_over_matter()));
}

#[test]
fn trait_and_kind_filter_needs_both() {
    let tome_asset = CardFilter::with_trait("Tome").and_kind(CardType::Asset);
    assert!(tome_asset.matches(&old_book_of_lore()));
    // An asset without the trait.
    assert!(!tome_asset.matches(&research_librarian()));
    // The trait on a card of another type.
    let tome_event = CardMetadata {
        traits: vec!["Tome".into()],
        ..mind_over_matter()
    };
    assert!(!tome_asset.matches(&tome_event));
}

#[test]
fn empty_filter_matches_everything() {
    let any = CardFilter {
        trait_: None,
        kind: None,
    };
    assert!(any.matches(&old_book_of_lore()));
    assert!(any.matches(&research_librarian()));
    assert!(any.matches(&mind_over_matter()));
}

#[test]
fn constructors_set_exactly_the_named_fields() {
    assert_eq!(
        CardFilter::with_trait("Tome"),
        CardFilter {
            trait_: Some("Tome".to_owned()),
            kind: None,
        }
    );
    assert_eq!(
        CardFilter::of_kind(CardType::Asset),
        CardFilter {
            trait_: None,
            kind: Some(CardType::Asset),
        }
    );
    let both = CardFilter {
        trait_: Some("Tome".to_owned()),
        kind: Some(CardType::Asset),
    };
    assert_eq!(
        CardFilter::with_trait("Tome").and_kind(CardType::Asset),
        both
    );
    assert_eq!(CardFilter::of_kind(CardType::Asset).and_trait("Tome"), both);
}
