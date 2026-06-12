//! C1a end-to-end: the-gathering `setup()` seats a roster at the Study and
//! reaches a resolution; the Attic/Cellar forced-on-enter abilities fire
//! through the real card registry. Own process so it can install the
//! process-global registries against the real `cards` corpus.

use std::sync::Once;

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome};
use game_core::scenario::Resolution;
use game_core::state::{CardCode, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    fire_forced_on_enter, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, PlayerAction};
use scenarios::{the_gathering, REGISTRY};

static INSTALL: Once = Once::new();

fn install_registries() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Apply one action, asserting it is not `Rejected`.
fn apply_checked(
    state: game_core::state::GameState,
    action: &Action,
) -> game_core::state::GameState {
    let r = apply(state, action.clone());
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "action {action:?} was rejected: {:?}",
        r.outcome,
    );
    r.state
}

/// Seat solo Roland (01001, empty deck) and close the mulligan window.
fn setup_and_seat() -> game_core::state::GameState {
    let inv = InvestigatorId(1);
    let mut state = the_gathering::setup();
    for a in [
        Action::Player(PlayerAction::StartScenario {
            roster: vec![RosterEntry {
                investigator: CardCode("01001".into()),
                deck: vec![],
            }],
        }),
        Action::Player(PlayerAction::Mulligan {
            investigator: inv,
            indices_to_redraw: vec![],
        }),
    ] {
        state = apply_checked(state, &a);
    }
    state
}

#[test]
fn roster_seating_places_investigator_at_study() {
    install_registries();
    let state = setup_and_seat();
    let roland = state
        .investigators
        .get(&InvestigatorId(1))
        .expect("Roland seated");
    let study = state.starting_location;
    assert_eq!(
        roland.current_location, study,
        "seating must place investigators at setup()'s starting_location",
    );
}

#[test]
fn drives_to_a_won_resolution() {
    install_registries();
    let inv = InvestigatorId(1);
    let mut state = setup_and_seat();

    // Hand the investigator enough clues to clear acts 1 and 2 (2 + 3 = 5
    // needed); act 3 (01110) has threshold 0 — it advances on Ghoul-Priest-
    // defeat, not a clue spend. 7 is comfortably enough. AdvanceAct spends
    // from group clues, no chaos draw involved. Proves the resolution latch
    // fires for the real act deck, deterministically.
    state.investigators.get_mut(&inv).unwrap().clues = 7;

    for _ in 0..3 {
        state = apply_checked(
            state,
            &Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        );
    }

    assert!(
        matches!(state.resolution, Some(Resolution::Won { .. })),
        "advancing through the terminal act latches Won, got {:?}",
        state.resolution,
    );
}

#[test]
fn attic_forced_enter_deals_one_horror() {
    install_registries();
    // A bare board with the Attic (01113); fire the forced
    // EnteredLocation trigger directly via the test helper (live entry
    // isn't reachable until C1b's Door-on-the-Floor transition).
    let mut attic = test_location(20, "Attic");
    attic.code = CardCode("01113".into());
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(20))
        .with_location(attic)
        .build();
    let mut events = Vec::new();

    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(20));
    assert!(matches!(outcome, EngineOutcome::Done));
    assert_eq!(
        state.investigators.get(&InvestigatorId(1)).unwrap().horror,
        1,
        "entering the Attic deals 1 horror to the entering investigator",
    );
}

#[test]
fn cellar_forced_enter_deals_one_damage() {
    install_registries();
    let mut cellar = test_location(21, "Cellar");
    cellar.code = CardCode("01114".into());
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(21))
        .with_location(cellar)
        .build();
    let mut events = Vec::new();

    let outcome = fire_forced_on_enter(&mut state, &mut events, InvestigatorId(1), LocationId(21));
    assert!(matches!(outcome, EngineOutcome::Done));
    assert_eq!(
        state.investigators.get(&InvestigatorId(1)).unwrap().damage,
        1,
        "entering the Cellar deals 1 damage to the entering investigator",
    );
}

#[test]
fn advancing_act_1_rebuilds_the_board() {
    install_registries();
    let mut state = the_gathering::setup();

    // Seat one investigator at the Study with the 2 clues Act 1 needs.
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.current_location = state.starting_location;
    investigator.clues = 2;
    state.investigators.insert(inv, investigator);
    state.turn_order = vec![inv];
    state.active_investigator = Some(inv);
    state.phase = Phase::Investigation;

    let result = apply(
        state,
        Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);

    // Board rebuilt: four locations in play, Study gone, set-aside empty.
    let codes: std::collections::BTreeSet<String> = result
        .state
        .locations
        .values()
        .map(|l| l.code.as_str().to_owned())
        .collect();
    assert_eq!(
        codes,
        ["01112", "01113", "01114", "01115"]
            .into_iter()
            .map(String::from)
            .collect()
    );
    assert!(result.state.set_aside_locations.is_empty());

    // Investigator relocated to the Hallway (01112).
    let hallway_id = result
        .state
        .locations
        .values()
        .find(|l| l.code.as_str() == "01112")
        .unwrap()
        .id;
    assert_eq!(
        result.state.investigators[&inv].current_location,
        Some(hallway_id)
    );

    // Act cursor moved to Act 2.
    assert_eq!(result.state.act_index, 1);
}
