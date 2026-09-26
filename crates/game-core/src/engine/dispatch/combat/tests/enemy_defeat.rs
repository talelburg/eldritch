use super::*;

#[test]
fn defeating_victory_enemy_places_it_in_the_victory_display() {
    let eid = EnemyId(1);
    let mut enemy = test_support::test_enemy(1, "Ghoul Priest");
    enemy.code = CardCode::new("01116");
    enemy.max_health = 1;
    enemy.victory = Some(2);
    let mut state = GameStateBuilder::new().build();
    state.enemies.insert(eid, enemy);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

    assert_eq!(state.victory_display, vec![CardCode::new("01116")]);
    assert_event!(
        events,
        Event::EnteredVictoryDisplay { code, victory: 2 } if code.as_str() == "01116"
    );
    // "place the card in the victory display instead of in the discard
    // pile" (`glossary/Victory_Display_Victory_Points.md`) — the victory
    // display is the only pile it lands in (#632).
    assert!(
        state.encounter_discard.is_empty(),
        "a victory enemy goes to the victory display instead of the encounter discard"
    );
}

#[test]
fn defeating_non_victory_enemy_places_its_card_in_the_encounter_discard() {
    // `glossary/Defeat.md`: "that enemy is defeated and placed on the
    // encounter discard pile" — not removed from the game, so the
    // `glossary/Encounter_Deck.md` reshuffle can bring it back (#632).
    let eid = EnemyId(1);
    let mut enemy = test_support::test_enemy(1, "Ghoul");
    enemy.code = CardCode::new("01160");
    enemy.max_health = 1;
    enemy.victory = None;
    let mut state = GameStateBuilder::new().build();
    state.enemies.insert(eid, enemy);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

    assert!(state.victory_display.is_empty());
    assert_no_event!(events, Event::EnteredVictoryDisplay { .. });
    assert_eq!(
        state.encounter_discard,
        vec![CardCode::new("01160")],
        "defeated non-victory enemy lands in the encounter discard"
    );
}

#[test]
fn defeating_enemy_without_registry_still_removes_it() {
    let eid = EnemyId(1);
    let mut enemy = test_support::test_enemy(1, "Ghoul");
    enemy.max_health = 1;
    let mut state = GameStateBuilder::new().build();
    state.enemies.insert(eid, enemy);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));
    assert!(!state.enemies.contains_key(&eid), "defeated enemy removed");
}
