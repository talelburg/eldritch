//! [`LiveGame`] driven from outside: built from a staged board and its settled
//! prompt, then submitted to and committed.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, location, seat, TEST_REGISTRIES};
use protocol::live::{LiveGame, Refused};
use protocol::logged_answer::LoggedResponse;
use protocol::{Applied, LogPosition, ServerMessage};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The position the [`at_turn_menu`] game's open turn waits at: three answers
/// are already in its log.
const TURN_MENU: LogPosition = LogPosition(3);

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
        investigator: INV,
        response: InputResponse::PickSingle(OptionId(
            u32::try_from(idx).expect("an action index fits u32"),
        )),
    }
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&INV].resources
}

/// The payload of the `Applied` a commit hands back.
fn applied(message: ServerMessage) -> Applied {
    let ServerMessage::Applied(applied) = message else {
        panic!("expected Applied, got {message:?}");
    };
    applied
}

#[test]
fn a_legal_answer_is_accepted_with_its_logged_answer_and_applied() {
    let mut live = at_turn_menu();
    let action = take_a_resource(live.state());
    let PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(picked),
        ..
    } = action
    else {
        unreachable!("take_a_resource picks one option");
    };

    let accepted = live
        .submit(TURN_MENU, action)
        .expect("the engine accepts it");

    let LoggedResponse::PickSingle { option } = &accepted.answer().response else {
        panic!("logged as {:?}", accepted.answer().response);
    };
    assert_eq!(
        option.id, picked,
        "the logged answer names the option picked"
    );
    let draws = accepted.draws();
    let applied = applied(live.commit(accepted));
    assert_eq!(
        resources(&applied.state),
        4,
        "the Applied holds the next state"
    );
    assert_eq!(
        draws, applied.state.rng.draws,
        "the draws checkpoint is the next state's"
    );
}

#[test]
fn submitting_changes_nothing_until_commit() {
    let live = at_turn_menu();
    let before = live.state().clone();
    let outcome_before = live.outcome().clone();

    let _accepted = live
        .submit(TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");

    assert_eq!(*live.state(), before);
    assert_eq!(*live.outcome(), outcome_before);
    assert_eq!(live.prompt(), TURN_MENU);
}

#[test]
fn commit_advances_to_the_applied_state() {
    let mut live = at_turn_menu();
    let accepted = live
        .submit(TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");

    let applied = applied(live.commit(accepted));

    assert_eq!(live.prompt(), LogPosition(4));
    assert_eq!(*live.state(), *applied.state);
    assert_eq!(*live.outcome(), applied.outcome);
    assert_eq!(resources(live.state()), 4);
}

#[test]
fn an_applied_names_the_position_it_answered_and_the_next_prompt() {
    let mut live = at_turn_menu();
    let accepted = live
        .submit(TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts it");

    let applied = applied(live.commit(accepted));

    assert_eq!(applied.answered, LogPosition(3));
    assert_eq!(applied.prompt, LogPosition(4));
}

#[test]
fn an_answer_the_engine_rejects_is_refused() {
    let live = at_turn_menu();

    let refused = live
        .submit(
            TURN_MENU,
            PlayerAction::ResolveInput {
                investigator: INV,
                response: InputResponse::PickSingle(OptionId(999)),
            },
        )
        .expect_err("no option 999 is offered");

    assert!(matches!(refused, Refused::Engine(_)), "got {refused:?}");
}

#[test]
fn an_answer_to_an_earlier_position_is_refused_as_a_stale_prompt() {
    let live = at_turn_menu();

    let refused = live
        .submit(LogPosition(2), take_a_resource(live.state()))
        .expect_err("position 2 was answered already");

    assert_eq!(refused, Refused::StalePrompt);
}

#[test]
fn an_in_range_option_from_a_second_tab_on_the_old_prompt_is_refused() {
    // Tab A takes a resource; tab B, still showing that same turn menu, then
    // picks the same option. It is still in range on the new turn menu, so
    // only the position tells the two prompts apart.
    let mut live = at_turn_menu();
    let tab_b = take_a_resource(live.state());
    let tab_a = live
        .submit(TURN_MENU, take_a_resource(live.state()))
        .expect("the engine accepts tab A's answer");
    let _ = live.commit(tab_a);

    let refused = live
        .submit(TURN_MENU, tab_b)
        .expect_err("tab B answered the prompt tab A already answered");

    assert_eq!(refused, Refused::StalePrompt);
}

#[test]
fn hello_carries_the_build_current_position_state_outcome_and_setup_events() {
    let live = at_turn_menu();

    let ServerMessage::Hello {
        build,
        prompt,
        state,
        outcome,
        events,
    } = live.hello()
    else {
        panic!("expected Hello");
    };

    assert_eq!(build, protocol::build_id());
    assert_eq!(prompt, TURN_MENU);
    assert_eq!(*state, *live.state());
    assert_eq!(outcome, *live.outcome());
    assert_eq!(events, vec![Event::ScenarioStarted]);
}
