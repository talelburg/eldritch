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
use game_core::{Action, InputResponse, PlayerAction};
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
    let mut state = the_gathering::setup();
    for a in [
        Action::Player(PlayerAction::StartScenario {
            roster: vec![RosterEntry {
                investigator: CardCode("01001".into()),
                deck: vec![],
            }],
        }),
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
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
    let study_loc = &state.locations[&state.starting_location.unwrap()];
    assert!(study_loc.revealed, "seating reveals the starting location");
    assert_eq!(study_loc.clues, 2, "1 investigator × 2 per-investigator");
}

#[test]
fn drives_act_1_then_act_2_via_round_end_window() {
    install_registries();
    let inv = InvestigatorId(1);
    let mut state = setup_and_seat();

    // Enough clues to clear act 1 (threshold 2, AdvanceAct) and then act 2
    // (threshold 3) via its round-end objective; a small deck so the upkeep
    // draw doesn't deck out. Act 3 (01110) advances on Ghoul-Priest-defeat —
    // covered by cards/tests/act_advancement.rs — so this test stops at
    // reaching act 3 through the real deck.
    {
        let i = state.investigators.get_mut(&inv).unwrap();
        i.clues = 7;
        i.deck = (0..5).map(|n| CardCode(format!("filler{n}"))).collect();
    }

    // Act 1: the normal Investigation-phase clue spend. Board builds and the
    // investigator relocates to the Hallway (01112) — the act-2 contributors.
    state = apply_checked(
        state,
        &Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
    );
    assert_eq!(state.act_index, 1, "act 1 advanced to act 2");

    // End the round: the cascade reaches step 4.6 and opens act 2's round-end
    // window (Hallway investigator holds >= 3 clues).
    let r = apply(state, Action::Player(PlayerAction::EndTurn));
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "round end opens the act-2 clue-spend window, got {:?}",
        r.outcome,
    );
    assert!(matches!(
        r.state.continuations.last(),
        Some(game_core::state::Continuation::TimingPointWindow {
            event: game_core::engine::TimingEvent::RoundEnded,
            mode: game_core::state::TimingMode::Reaction,
            ..
        })
    ));

    // Pick the act-advance candidate (the window's sole option): act 2 advances
    // to act 3 via 01109's `When`-RoundEnded group clue-spend (#434).
    let r = apply(
        r.state,
        Action::Player(PlayerAction::ResolveInput {
            response: game_core::action::InputResponse::PickSingle(game_core::engine::OptionId(0)),
        }),
    );
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "Confirm rejected: {:?}",
        r.outcome,
    );
    assert_eq!(r.state.act_index, 2, "act 2 advanced to act 3 (01110)");
    assert_eq!(
        r.state.act_deck[2].resolution,
        Some(Resolution::Won { id: "R1".into() }),
        "the terminal act carries the Won resolution (latched on Ghoul-Priest defeat)",
    );

    // Act 2's reverse (#280) fired on advance: the set-aside Ghoul Priest
    // (01116) is now in play in the Hallway (01112), and the Parlor (01115)
    // is revealed. This makes act 3's "Ghoul Priest defeated" objective
    // reachable in real play.
    assert!(
        r.state.set_aside_enemies.is_empty(),
        "the Ghoul Priest left the set-aside zone",
    );
    let priest = r
        .state
        .enemies
        .values()
        .find(|e| e.code.as_str() == "01116")
        .expect("Ghoul Priest (01116) spawned");
    let hallway_id = r
        .state
        .locations
        .values()
        .find(|l| l.code.as_str() == "01112")
        .unwrap()
        .id;
    assert_eq!(
        priest.current_location,
        Some(hallway_id),
        "the Ghoul Priest spawns in the Hallway",
    );
    let parlor = r
        .state
        .locations
        .values()
        .find(|l| l.code.as_str() == "01115")
        .expect("Parlor (01115) in play");
    assert!(parlor.revealed, "act 2's reverse reveals the Parlor");
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

    // Hallway (01112) was revealed when the investigator was relocated there.
    let hallway = result
        .state
        .locations
        .values()
        .find(|l| l.code.as_str() == "01112")
        .unwrap();
    assert!(hallway.revealed, "relocate-to-Hallway reveals it");
}
