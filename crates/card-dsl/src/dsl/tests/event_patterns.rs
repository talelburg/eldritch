use super::*;

/// `EventPattern::CardRevealed { card_type: Some(...) }` and
/// `{ card_type: None }` are distinct variants with serde
/// round-tripping. Locks the wire shape now so #52's persistence
/// doesn't surprise later.
#[test]
fn card_revealed_pattern_round_trips_through_serde_json() {
    let any = EventPattern::CardRevealed { card_type: None };
    let treachery = EventPattern::CardRevealed {
        card_type: Some(CardType::Treachery),
    };
    for original in [any, treachery] {
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }
}

#[test]
fn card_revealed_distinct_from_enemy_defeated() {
    let revealed_treachery = EventPattern::CardRevealed {
        card_type: Some(CardType::Treachery),
    };
    let enemy_defeated = EventPattern::EnemyDefeated {
        by_controller: true,
        code: None,
    };
    assert_ne!(revealed_treachery, enemy_defeated);
}

#[test]
fn enemy_spawned_pattern_round_trips_through_serde_json() {
    let original = EventPattern::EnemySpawned;
    let json = serde_json::to_string(&original).expect("serialize");
    let recovered: EventPattern = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, recovered);
}

#[test]
fn enemy_spawned_distinct_from_other_patterns() {
    let spawned = EventPattern::EnemySpawned;
    let defeated = EventPattern::EnemyDefeated {
        by_controller: true,
        code: None,
    };
    let revealed = EventPattern::CardRevealed { card_type: None };
    assert_ne!(spawned, defeated);
    assert_ne!(spawned, revealed);
}

#[test]
fn entered_location_pattern_round_trips() {
    let p = EventPattern::EnteredLocation;
    let json = serde_json::to_string(&p).unwrap();
    let back: EventPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn discover_clues_and_game_end_round_trip() {
    for p in [EventPattern::DiscoverClues, EventPattern::GameEnd] {
        let json = serde_json::to_string(&p).expect("serialize");
        let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }
}

#[test]
fn enemy_attack_damaged_self_round_trips() {
    let p = EventPattern::EnemyAttackDamagedSelf;
    let json = serde_json::to_string(&p).expect("serialize");
    let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(p, back);
}

#[test]
fn cancel_effect_and_enemy_attacks_pattern_round_trip() {
    let e = Effect::Cancel;
    let json = serde_json::to_string(&e).expect("serialize");
    assert_eq!(
        Effect::Cancel,
        serde_json::from_str(&json).expect("deserialize")
    );

    let p = EventPattern::EnemyAttacks;
    let json = serde_json::to_string(&p).expect("serialize");
    assert_eq!(
        EventPattern::EnemyAttacks,
        serde_json::from_str(&json).expect("deserialize")
    );
}

#[test]
fn skill_test_resolved_round_trips() {
    let p = EventPattern::SkillTestResolved {
        outcome: TestOutcome::Success,
        kind: Some(SkillTestKind::Investigate),
        by_controller: true,
    };
    let json = serde_json::to_string(&p).expect("serialize");
    let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(p, back);
}

/// The unqualified form — Lita Chantler 01117's *"When **an investigator**
/// at your location successfully attacks…"* — round-trips as its own
/// value, so a `false` is carried rather than defaulted back to the
/// corpus-common `true`.
#[test]
fn skill_test_resolved_unqualified_round_trips() {
    let p = EventPattern::SkillTestResolved {
        outcome: TestOutcome::Success,
        kind: Some(SkillTestKind::Fight),
        by_controller: false,
    };
    let json = serde_json::to_string(&p).expect("serialize");
    let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(p, back);
    assert_ne!(
        p,
        EventPattern::SkillTestResolved {
            outcome: TestOutcome::Success,
            kind: Some(SkillTestKind::Fight),
            by_controller: true,
        }
    );
}

#[test]
fn phase_ended_pattern_round_trips() {
    let p = EventPattern::PhaseEnded {
        phase: Phase::Enemy,
    };
    let json = serde_json::to_string(&p).unwrap();
    let back: EventPattern = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn enemy_defeated_carries_optional_code_narrow() {
    let any = EventPattern::EnemyDefeated {
        by_controller: false,
        code: None,
    };
    let narrowed = EventPattern::EnemyDefeated {
        by_controller: false,
        code: Some("01116".into()),
    };
    assert_ne!(any, narrowed);
}
