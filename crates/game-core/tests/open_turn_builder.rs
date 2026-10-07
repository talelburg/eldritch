//! The builder's open-turn and engagement fixtures (#939) produce states the
//! engine treats as a real open turn and a real engagement.
//!
//! Asserted only on what a player could observe — the prompt the engine
//! surfaces, events, and the board — never on the continuation stack the
//! builder assembles.
//!
//! Synthetic probes only (ADR 0016): the stock test investigator, location and
//! enemy, none of which stands in for a printed card.

use game_core::assert_event_sequence;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::event::Event;
use game_core::state::{GameState, GameStateBuilder, InvestigatorId, LocationId, Phase};
use game_core::test_support;

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_test_registry();
}

const MINE: InvestigatorId = InvestigatorId(1);
const OTHER: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);

/// The prompt anchor of `outcome`, or a panic naming what came back instead.
fn prompt_anchor(outcome: &EngineOutcome) -> Option<OptionTarget> {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected a prompt, got {outcome:?}");
    };
    request.target.clone()
}

/// One basic action taken from an `open_turn` state comes back to the turn
/// menu, anchored to the acting investigator's turn control — the engine
/// recognises the built state as that investigator's open turn.
#[test]
fn an_open_turn_resumes_at_the_turn_menu_after_an_action() {
    let state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(MINE)
        .build();

    let result =
        test_support::take_turn_action(state, &TurnAction::Resource { investigator: MINE });

    assert_eq!(
        prompt_anchor(&result.outcome),
        Some(OptionTarget::TurnControl(MINE)),
    );
    assert_eq!(result.state.phase, Phase::Investigation);
    assert_eq!(result.state.active_investigator, Some(MINE));
}

/// With no turn order set, `open_turn` seats the investigator alone.
#[test]
fn open_turn_defaults_the_turn_order_to_the_investigator() {
    let state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(MINE)
        .build();

    assert_eq!(state.turn_order, vec![MINE]);
}

/// A turn order set before `open_turn` stands: ending the first investigator's
/// turn hands the turn to the second, whose menu comes up next.
#[test]
fn open_turn_respects_a_turn_order_already_set() {
    let state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), HERE)
        .with_investigator_at(test_support::test_investigator(2), HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_turn_order([MINE, OTHER])
        .open_turn(MINE)
        .build();

    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);

    assert_eq!(
        prompt_anchor(&result.outcome),
        Some(OptionTarget::TurnControl(OTHER)),
    );
}

/// An investigator engaged with one ready enemy, at a location the enemy was
/// never explicitly placed at.
fn engaged_board() -> GameState {
    let mut attacker = test_support::test_enemy(7, "Attacker");
    attacker.attack_damage = 2;
    attacker.attack_horror = 0;
    attacker.max_health = 5;

    GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), HERE)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_enemy_engaged(attacker, MINE)
        .open_turn(MINE)
        .build()
}

/// `with_enemy_engaged` puts the enemy where its investigator stands.
#[test]
fn an_engaged_enemy_is_at_its_investigators_location() {
    let state = engaged_board();
    let enemy = state.enemies.values().next().expect("the attacker");
    assert_eq!(enemy.engaged_with, Some(MINE));
    assert_eq!(enemy.current_location, Some(HERE));
}

/// A provoking action taken while engaged draws the enemy's attack of
/// opportunity — its damage lands before the action resolves.
#[test]
fn an_engaged_enemy_attacks_of_opportunity_when_a_provoking_action_is_taken() {
    let result = test_support::take_turn_action(
        engaged_board(),
        &TurnAction::Resource { investigator: MINE },
    );

    assert_event_sequence!(
        result.events,
        Event::DamageTaken {
            investigator: MINE,
            amount: 2
        },
        Event::ResourcesGained {
            investigator: MINE,
            ..
        },
    );
    assert_eq!(result.state.investigators[&MINE].damage(), 2);
}
