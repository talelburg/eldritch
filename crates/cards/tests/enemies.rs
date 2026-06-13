//! C3b — the six Gathering encounter enemies carry their printed stats,
//! keywords, and spawn-location in the corpus. Stats verified against
//! `data/arkhamdb-snapshot/pack/core/core_encounter.json`.

use card_dsl::card_data::{
    CardKind, HealthValue, Prey, PreyDirection, PreyMeasure, SkillKind, Spawn, SpawnLocation,
};

fn enemy(code: &str) -> CardKind {
    cards::by_code(code)
        .unwrap_or_else(|| panic!("enemy {code} in corpus"))
        .kind
        .clone()
}

#[test]
fn ghoul_priest_full_profile() {
    let CardKind::Enemy {
        fight,
        evade,
        damage,
        horror,
        health,
        victory,
        hunter,
        retaliate,
        prey,
        ..
    } = enemy("01116")
    else {
        panic!("01116 is an enemy")
    };
    assert_eq!((fight, evade, damage, horror), (4, 4, 2, 2));
    assert_eq!(health, Some(HealthValue::PerInvestigator(5)));
    assert_eq!(victory, Some(2));
    assert!(hunter);
    assert!(retaliate);
    assert_eq!(
        prey,
        Prey::Ranked {
            direction: PreyDirection::Highest,
            measure: PreyMeasure::Skill(SkillKind::Combat),
        },
    );
}

#[test]
fn flesh_eater_spawns_at_attic() {
    let CardKind::Enemy {
        fight,
        evade,
        damage,
        horror,
        health,
        spawn,
        ..
    } = enemy("01118")
    else {
        panic!("enemy")
    };
    assert_eq!((fight, evade, damage, horror), (4, 1, 1, 2));
    assert_eq!(health, Some(HealthValue::Fixed(4)));
    assert_eq!(
        spawn,
        Some(Spawn {
            location: SpawnLocation::Specific("01113".to_owned()),
        }),
    );
}

#[test]
fn icy_ghoul_spawns_at_cellar() {
    let CardKind::Enemy {
        fight,
        evade,
        damage,
        horror,
        spawn,
        ..
    } = enemy("01119")
    else {
        panic!("enemy")
    };
    assert_eq!((fight, evade, damage, horror), (3, 4, 2, 1));
    assert_eq!(
        spawn,
        Some(Spawn {
            location: SpawnLocation::Specific("01114".to_owned()),
        }),
    );
}

#[test]
fn ghoul_minion_is_plain() {
    let CardKind::Enemy {
        fight,
        evade,
        damage,
        horror,
        health,
        hunter,
        retaliate,
        prey,
        quantity,
        ..
    } = enemy("01160")
    else {
        panic!("enemy")
    };
    assert_eq!((fight, evade, damage, horror), (2, 2, 1, 1));
    assert_eq!(health, Some(HealthValue::Fixed(2)));
    assert!(!hunter);
    assert!(!retaliate);
    assert_eq!(prey, Prey::Default);
    assert_eq!(quantity, 3);
}

#[test]
fn ravenous_ghoul_prey_lowest_remaining_health() {
    let CardKind::Enemy {
        fight,
        evade,
        damage,
        horror,
        prey,
        ..
    } = enemy("01161")
    else {
        panic!("enemy")
    };
    assert_eq!((fight, evade, damage, horror), (3, 3, 1, 1));
    assert_eq!(
        prey,
        Prey::Ranked {
            direction: PreyDirection::Lowest,
            measure: PreyMeasure::RemainingHealth,
        },
    );
}

#[test]
fn swarm_of_rats_is_a_hunter() {
    let CardKind::Enemy {
        fight,
        evade,
        damage,
        horror,
        hunter,
        quantity,
        ..
    } = enemy("01159")
    else {
        panic!("enemy")
    };
    assert_eq!((fight, evade, damage, horror), (1, 3, 1, 0));
    assert!(hunter);
    assert_eq!(quantity, 3);
}
