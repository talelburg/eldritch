use card_dsl::card_data::{CardKind, CardMetadata, ClueValue, Prey};

use super::*;
use crate::state::{CardCode, GameStateBuilder, Location, LocationId};

fn location_meta(code: &str, name: &str, shroud: u8, clues: u8) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: name.to_string(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".to_string(),
        weakness: false,
        kind: CardKind::Location {
            shroud,
            printed_clues: ClueValue::PerInvestigator(clues),
            victory: None,
        },
    }
}

#[test]
fn add_location_mints_sequential_ids_and_extracts_metadata() {
    let mut state = GameStateBuilder::new().build();
    let a = state.add_location(&location_meta("01111", "Study", 2, 2));
    let b = state.add_location(&location_meta("01112", "Hallway", 1, 0));
    assert_ne!(a, b, "ids are distinct");
    let study = &state.locations[&a];
    assert_eq!(study.code.as_str(), "01111");
    assert_eq!(study.name, "Study");
    assert_eq!(study.shroud, 2);
    assert_eq!(study.clues, 0, "enters unrevealed with no clues");
    assert!(!study.revealed);
    assert_eq!(study.printed_clues, ClueValue::PerInvestigator(2));
    assert!(study.connections.is_empty());
    assert_eq!(state.location_ids.peek(), 2, "counter advanced twice");
}

#[test]
fn add_set_aside_card_records_a_location_by_code_only() {
    let mut state = GameStateBuilder::new().build();
    state.add_set_aside_card(&location_meta("01113", "Attic", 1, 2));
    assert_eq!(state.set_aside_cards, vec![CardCode::new("01113")],);
    assert!(state.locations.is_empty(), "not in play");
    assert_eq!(
        state.location_ids.peek(),
        0,
        "no LocationId is minted until the location enters play",
    );
}

fn enemy_meta(code: &str, name: &str) -> CardMetadata {
    CardMetadata {
        code: code.to_string(),
        name: name.to_string(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".to_string(),
        weakness: false,
        kind: CardKind::Enemy {
            fight: 1,
            evade: 1,
            damage: 0,
            horror: 0,
            health: None,
            victory: None,
            spawn: None,
            surge: false,
            peril: false,
            hunter: false,
            retaliate: false,
            prey: Prey::Default,
            quantity: 1,
        },
    }
}

#[test]
fn add_set_aside_card_holds_locations_and_enemies_in_one_zone() {
    // The point of the single zone: two cardtypes, one collection, one
    // representation. Which one a code is gets read back from the
    // metadata at put-into-play time.
    let mut state = GameStateBuilder::new().build();
    state.add_set_aside_card(&location_meta("01113", "Attic", 1, 2));
    state.add_set_aside_card(&enemy_meta("01116", "Ghoul Priest"));
    assert_eq!(
        state.set_aside_cards,
        vec![CardCode::new("01113"), CardCode::new("01116"),],
    );
}

#[test]
#[should_panic(expected = "not a Location")]
fn add_location_panics_on_non_location_metadata() {
    let mut state = GameStateBuilder::new().build();
    let meta = CardMetadata {
        code: "01108".to_string(),
        name: "Trapped".to_string(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "core".to_string(),
        weakness: false,
        kind: CardKind::Act {
            clue_threshold: Some(2),
            victory: None,
        },
    };
    state.add_location(&meta);
}

#[test]
fn connect_wires_both_directions() {
    let mut state = GameStateBuilder::new()
        .with_location(Location::new(
            LocationId(1),
            CardCode("a".into()),
            "A",
            1,
            0,
        ))
        .with_location(Location::new(
            LocationId(2),
            CardCode("b".into()),
            "B",
            1,
            0,
        ))
        .build();
    state.connect(LocationId(1), LocationId(2));
    assert_eq!(
        state.locations[&LocationId(1)].connections,
        vec![LocationId(2)]
    );
    assert_eq!(
        state.locations[&LocationId(2)].connections,
        vec![LocationId(1)]
    );
}

#[test]
#[should_panic(expected = "connect: location LocationId(2) not found")]
fn connect_panics_on_a_location_that_is_not_in_play() {
    // A set-aside card has no `LocationId` at all now, so `connect` has
    // one zone to search. Layout wiring happens at entry instead.
    let mut state = GameStateBuilder::new()
        .with_location(Location::new(
            LocationId(1),
            CardCode("a".into()),
            "A",
            1,
            0,
        ))
        .build();
    state.connect(LocationId(2), LocationId(1));
}

#[test]
fn game_state_starting_location_defaults_to_none_and_roundtrips() {
    let mut state = GameStateBuilder::new().build();
    assert_eq!(state.starting_location, None, "default must be None");

    state.starting_location = Some(LocationId(7));
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.starting_location, Some(LocationId(7)));
}
