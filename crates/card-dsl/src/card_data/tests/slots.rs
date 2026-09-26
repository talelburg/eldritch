use super::*;

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
