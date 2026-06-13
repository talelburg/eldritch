//! Agenda 01107 forced abilities through the real card registry: the
//! enemy-phase-end move + round-end doom fire via the forced-trigger
//! path and the `Effect::Native` bridge end-to-end.

use std::sync::OnceLock;

use game_core::state::{
    Agenda, CardCode, Enemy, EnemyId, GameState, InvestigatorId, Location, LocationId, Phase,
};
use game_core::test_support::{
    fire_forced_on_phase_end, fire_forced_on_round_end, test_enemy, test_investigator,
    GameStateBuilder,
};
use game_core::{EngineOutcome, Event};

fn install() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

fn ghoul(id: u32, at: LocationId) -> Enemy {
    let mut e = test_enemy(id, "Ghoul");
    e.traits = vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()];
    e.current_location = Some(at);
    e
}

fn board_with_agenda() -> GameState {
    let loc = |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .with_location(loc(2, "01112", "Hallway"))
        .with_location(loc(5, "01115", "Parlor"))
        .with_phase(Phase::Enemy)
        .build();
    state.connect(LocationId(2), LocationId(5));
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn enemy_phase_end_moves_ghoul_toward_parlor() {
    install();
    let mut state = board_with_agenda();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(5)),
        "Ghoul stepped Hallway -> Parlor"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::EnemyMoved { to, .. } if *to == LocationId(5))));
}

#[test]
fn round_end_places_doom_per_ghoul_in_hallway_or_parlor() {
    install();
    let mut state = board_with_agenda();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2)));
    state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5)));
    let mut events = Vec::new();
    let outcome = fire_forced_on_round_end(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 2, "1 doom per Ghoul in Hallway/Parlor");
}
