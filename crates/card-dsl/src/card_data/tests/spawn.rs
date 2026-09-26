use super::*;

#[test]
fn spawn_specific_round_trips_through_serde_json() {
    let original = Spawn {
        location: SpawnLocation::Specific("01112".to_owned()),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let recovered: Spawn = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, recovered);
}

#[test]
fn health_value_serde_roundtrip() {
    for hv in [HealthValue::Fixed(4), HealthValue::PerInvestigator(5)] {
        let json = serde_json::to_string(&hv).expect("serialize");
        assert_eq!(
            serde_json::from_str::<HealthValue>(&json).expect("deserialize"),
            hv
        );
    }
}

#[test]
fn card_metadata_serde_roundtrip_preserves_spawn_specific() {
    let original = CardMetadata {
        code: "_synth_enemy".into(),
        name: "Synth Enemy".into(),
        text: None,
        traits: Vec::new(),
        back_name: None,
        back_text: None,
        pack_code: "_synth".into(),
        weakness: false,
        kind: CardKind::Enemy {
            fight: 3,
            evade: 2,
            damage: 1,
            horror: 1,
            health: Some(HealthValue::Fixed(1)),
            victory: None,
            spawn: Some(Spawn {
                location: SpawnLocation::Specific("_synth_loc".into()),
            }),
            surge: false,
            peril: false,
            hunter: false,
            retaliate: false,
            prey: Prey::Default,
            quantity: 1,
        },
    };
    let json = serde_json::to_string(&original).expect("serialize");
    let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn card_metadata_serde_roundtrip_preserves_spawn_none() {
    let original = CardMetadata {
        code: "01000".into(),
        name: "Random Basic Weakness".into(),
        text: None,
        traits: Vec::new(),
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
    let json = serde_json::to_string(&original).expect("serialize");
    let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
    assert!(matches!(back.kind, CardKind::Treachery { .. }));
}
