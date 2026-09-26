use super::*;

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
