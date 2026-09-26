use super::*;

#[test]
fn assign_attack_fills_soaker_before_investigator() {
    // 1 ally with remaining health 3, attack deals 2 damage / 0 horror →
    // all 2 damage soaks onto the ally, none on the investigator.
    let inst = CardInstanceId(7);
    let soakers = [Soaker {
        instance: inst,
        remaining_health: 3,
        remaining_sanity: 1,
    }];
    let assignment = assign_attack(&soakers, 2, 0);
    assert_eq!(assignment.investigator_damage, 0);
    assert_eq!(assignment.investigator_horror, 0);
    assert_eq!(assignment.asset_damage.get(&inst), Some(&2));
    assert!(assignment.asset_horror.is_empty());
}

#[test]
fn assign_attack_overflows_to_investigator_past_capacity() {
    // Ally with remaining health 1, attack deals 2 damage → 1 soaks onto
    // the ally, 1 overflows onto the investigator.
    let inst = CardInstanceId(7);
    let soakers = [Soaker {
        instance: inst,
        remaining_health: 1,
        remaining_sanity: 0,
    }];
    let assignment = assign_attack(&soakers, 2, 0);
    assert_eq!(assignment.asset_damage.get(&inst), Some(&1));
    assert_eq!(assignment.investigator_damage, 1);
    // Horror side trivially zero (attack deals no horror) — asserted so
    // the test is a complete contract, not a damage-only partial.
    assert_eq!(assignment.investigator_horror, 0);
    assert!(assignment.asset_horror.is_empty());
}

#[test]
fn place_assignment_accumulates_on_asset_and_investigator() {
    // Pre-construct an Assignment placing 1 damage + 1 horror on an
    // in-play asset and 1 damage on the investigator. Registry installed
    // so max_health() / max_sanity() can resolve; TEST_INV = 8/8 and the
    // investigator damage is 1 < 8, so no defeat fires.
    // Asset defeat-on-overflow needs the real `cards` registry and is
    // covered by the EU5 integration test.
    test_support::install_test_registry();

    let id = InvestigatorId(1);
    let inst = CardInstanceId(7);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play = vec![CardInPlay::enter_play(CardCode::new("01021"), inst)];

    let mut state = GameStateBuilder::new().with_investigator(inv).build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    let mut asset_damage = BTreeMap::new();
    asset_damage.insert(inst, 1u8);
    let mut asset_horror = BTreeMap::new();
    asset_horror.insert(inst, 1u8);
    let assignment = Assignment {
        investigator_damage: 1,
        investigator_horror: 0,
        asset_damage,
        asset_horror,
    };

    place_assignment(&mut cx, id, &assignment);

    let card = &state.investigators[&id].cards_in_play[0];
    assert_eq!(card.accumulated_damage, 1, "asset soaked 1 damage");
    assert_eq!(card.accumulated_horror, 1, "asset soaked 1 horror");
    assert_eq!(
        state.investigators[&id].damage(),
        1,
        "investigator took overflow damage"
    );
    assert_event!(events, Event::DamageTaken { investigator, amount: 1 } if *investigator == id);
}

#[test]
fn assign_attack_soaks_damage_and_horror_independently() {
    // Two soakers: A has only health, B has only sanity. Attack 1/1 →
    // damage to A, horror to B, nothing to the investigator.
    let a = CardInstanceId(1);
    let b = CardInstanceId(2);
    let soakers = [
        Soaker {
            instance: a,
            remaining_health: 2,
            remaining_sanity: 0,
        },
        Soaker {
            instance: b,
            remaining_health: 0,
            remaining_sanity: 2,
        },
    ];
    let assignment = assign_attack(&soakers, 1, 1);
    assert_eq!(assignment.asset_damage.get(&a), Some(&1));
    assert!(!assignment.asset_damage.contains_key(&b));
    assert_eq!(assignment.asset_horror.get(&b), Some(&1));
    assert!(!assignment.asset_horror.contains_key(&a));
    assert_eq!(assignment.investigator_damage, 0);
    assert_eq!(assignment.investigator_horror, 0);
}

#[test]
fn damage_application_accumulates_on_the_investigator_card() {
    test_support::install_test_registry();
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let defeated = apply_damage_numeric(&mut cx, id, 3);
    assert_eq!(
        state.investigators[&id]
            .investigator_card
            .accumulated_damage,
        3,
        "damage must accumulate on the investigator_card, not the legacy field"
    );
    assert_eq!(
        state.investigators[&id].damage(),
        3,
        "damage() accessor must read from investigator_card"
    );
    assert!(!defeated, "3 < 8 health — investigator not defeated");
}
