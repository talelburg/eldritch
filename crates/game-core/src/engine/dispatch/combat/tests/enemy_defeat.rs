use super::*;
use crate::test_support::enemy;

/// A stock enemy printed with victory points.
const VICTORY_ENEMY: &str = "_victory_enemy";
/// A stock enemy printed with no victory points.
const PLAIN_ENEMY: &str = "_plain_enemy";

#[test]
fn defeating_victory_enemy_places_it_in_the_victory_display() {
    let eid = EnemyId(1);
    let mut state = GameStateBuilder::new()
        .with_enemy(enemy(1).code(VICTORY_ENEMY).health(1).victory(2))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

    assert_eq!(state.victory_display, vec![CardCode::new(VICTORY_ENEMY)]);
    assert_event!(
        events,
        Event::EnteredVictoryDisplay { code, victory: 2 } if code.as_str() == VICTORY_ENEMY
    );
    // "place the card in the victory display instead of in the discard
    // pile" (`glossary/Victory_Display_Victory_Points.md`) — the victory
    // display is the only pile it lands in.
    assert!(
        state.encounter_discard.is_empty(),
        "a victory enemy goes to the victory display instead of the encounter discard"
    );
}

/// Regression for #632: a defeated enemy without victory points goes to the
/// encounter discard pile rather than out of the game.
#[test]
fn defeating_non_victory_enemy_places_its_card_in_the_encounter_discard() {
    // `glossary/Defeat.md`: "that enemy is defeated and placed on the
    // encounter discard pile" — not removed from the game, so the
    // `glossary/Encounter_Deck.md` reshuffle can bring it back.
    let eid = EnemyId(1);
    let mut state = GameStateBuilder::new()
        .with_enemy(enemy(1).code(PLAIN_ENEMY).health(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));

    assert!(state.victory_display.is_empty());
    assert_no_event!(events, Event::EnteredVictoryDisplay { .. });
    assert_eq!(
        state.encounter_discard,
        vec![CardCode::new(PLAIN_ENEMY)],
        "defeated non-victory enemy lands in the encounter discard"
    );
}

#[test]
fn defeating_enemy_with_no_card_metadata_still_removes_it() {
    let eid = EnemyId(1);
    let mut state = GameStateBuilder::new()
        .with_enemy(enemy(1).health(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    damage_enemy(&mut cx, eid, 1, Some(InvestigatorId(1)));
    assert!(!state.enemies.contains_key(&eid), "defeated enemy removed");
}
