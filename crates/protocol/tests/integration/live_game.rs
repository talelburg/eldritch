//! [`LiveGame`] driven from outside: built from a staged board and its settled
//! prompt, then submitted to and committed.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, location, seat, TEST_REGISTRIES};
use protocol::live::{LiveGame, Refusal};
use protocol::logged_answer::LoggedResponse;
use protocol::ServerMessage;

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// One investigator with 3 resources and their turn open, as a live game three
/// answers into its log.
fn at_turn_menu() -> LiveGame {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seat(1).at(HERE).resources(3))
        .open_turn(INV)
        .session();
    let outcome = test_support::awaiting_request(session.prompt().clone());
    LiveGame::new(
        session.state().clone(),
        outcome,
        vec![Event::ScenarioStarted],
        3,
        TEST_REGISTRIES,
    )
}

/// The turn menu's answer that takes a resource.
fn take_a_resource(state: &GameState) -> PlayerAction {
    let idx = enumerate::legal_actions(state, TEST_REGISTRIES.cards)
        .iter()
        .position(|a| *a == TurnAction::Resource { investigator: INV })
        .expect("taking a resource is legal at the open turn");
    PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(
            u32::try_from(idx).expect("an action index fits u32"),
        )),
    }
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&INV].resources
}

#[test]
fn a_legal_answer_is_accepted_with_its_logged_answer_and_applied() {
    let live = at_turn_menu();
    let action = take_a_resource(live.state());
    let PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(picked),
    } = action
    else {
        unreachable!("take_a_resource picks one option");
    };

    let accepted = live.submit(action).expect("the engine accepts it");

    let LoggedResponse::PickSingle { option } = &accepted.answer().response else {
        panic!("logged as {:?}", accepted.answer().response);
    };
    assert_eq!(
        option.id, picked,
        "the logged answer names the option picked"
    );
    let ServerMessage::Applied { state, .. } = accepted.applied() else {
        panic!("expected Applied, got {:?}", accepted.applied());
    };
    assert_eq!(resources(state), 4, "the Applied holds the next state");
    assert_eq!(accepted.draws(), state.rng.draws);
}

#[test]
fn submitting_changes_nothing_until_commit() {
    let live = at_turn_menu();
    let before = live.state().clone();
    let outcome_before = live.outcome().clone();

    let _accepted = live
        .submit(take_a_resource(live.state()))
        .expect("the engine accepts it");

    assert_eq!(*live.state(), before);
    assert_eq!(*live.outcome(), outcome_before);
    assert_eq!(live.seq(), 3);
}

#[test]
fn commit_advances_to_the_applied_state() {
    let mut live = at_turn_menu();
    let accepted = live
        .submit(take_a_resource(live.state()))
        .expect("the engine accepts it");

    let applied = live.commit(accepted);

    let ServerMessage::Applied { state, outcome, .. } = applied else {
        panic!("expected Applied, got {applied:?}");
    };
    assert_eq!(live.seq(), 4);
    assert_eq!(*live.state(), *state);
    assert_eq!(*live.outcome(), outcome);
    assert_eq!(resources(live.state()), 4);
}

#[test]
fn an_answer_the_engine_rejects_is_refused() {
    let live = at_turn_menu();

    let refusal = live
        .submit(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(999)),
        })
        .expect_err("no option 999 is offered");

    assert!(matches!(refusal, Refusal::Engine(_)), "got {refusal:?}");
}

#[test]
fn hello_carries_the_current_state_outcome_and_setup_events() {
    let live = at_turn_menu();

    let ServerMessage::Hello {
        state,
        outcome,
        events,
    } = live.hello()
    else {
        panic!("expected Hello");
    };

    assert_eq!(*state, *live.state());
    assert_eq!(outcome, *live.outcome());
    assert_eq!(events, vec![Event::ScenarioStarted]);
}
