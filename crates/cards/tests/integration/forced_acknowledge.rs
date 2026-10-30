//! A no-choice forced location ability (the Attic's 1 horror, the Cellar's
//! 1 damage) surfaces a one-option acknowledge *before* the harm lands when
//! `interactive_acknowledge` is on, and resolves synchronously when it is off.
//!
//! Runs against `cards::TEST_REGISTRIES`. The forced effect is driven through
//! the real `EnteredLocation` timing point by taking the Move into the location;
//! the interactive pause is then resumed by picking the acknowledge (the same
//! way a host resumes an `AwaitingInput`).

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, TestSession};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(2);
/// The location printing the forced on-enter ability under test.
const FORCED: LocationId = LocationId(1);

/// `INV`'s open turn at a plain location connected to one whose card `code`
/// carries a forced on-enter ability, with `interactive_acknowledge` set as
/// given; the session takes the Move into it.
fn enter(code: &str, interactive: bool) -> TestSession {
    let here = test_support::location(HERE).connections(vec![FORCED]);
    let forced = test_support::location(FORCED).as_card(code);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE))
        .with_location(here)
        .with_location(forced)
        .open_turn(INV)
        .build();
    state.interactive_acknowledge = interactive;
    TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::Move {
        investigator: INV,
        destination: FORCED,
    })
}

/// The acknowledge is a one-option pick anchored to the location on the map
/// rather than the flat bar.
fn assert_acknowledge_at_the_location(session: &TestSession) {
    let request = session.prompt();
    assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
    assert_eq!(
        session.offered_at(&OptionTarget::Location(FORCED)),
        1,
        "the forced-on-enter option anchors to the location on the map (#553), not the flat bar"
    );
}

#[test]
fn attic_forced_acknowledges_before_horror_when_interactive() {
    let paused = enter("01113", true); // the Attic — 1 horror
    assert_acknowledge_at_the_location(&paused);
    assert_eq!(
        paused.state().investigators[&INV].horror(),
        0,
        "horror must not be applied before the player acknowledges"
    );

    let session = paused.pick(OptionTarget::Location(FORCED));
    // Nothing is left pending after the acknowledge: the turn goes on.
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].horror(),
        1,
        "horror applied after the acknowledge"
    );
}

#[test]
fn attic_forced_resolves_synchronously_when_not_interactive() {
    let session = enter("01113", false);
    // Flag off: no suspend, the Move comes straight back to the turn menu.
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].horror(),
        1,
        "flag off: horror applied synchronously (today's behavior)"
    );
}

#[test]
fn cellar_forced_acknowledges_before_damage_when_interactive() {
    let paused = enter("01114", true); // the Cellar — 1 damage
    assert_acknowledge_at_the_location(&paused);
    assert_eq!(
        paused.state().investigators[&INV].damage(),
        0,
        "damage must not be applied before the player acknowledges"
    );

    let session = paused.pick(OptionTarget::Location(FORCED));
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "damage applied after the acknowledge"
    );
}

#[test]
fn cellar_forced_resolves_synchronously_when_not_interactive() {
    let session = enter("01114", false);
    // Flag off: no suspend, the Move comes straight back to the turn menu.
    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].damage(),
        1,
        "flag off: damage applied synchronously (today's behavior)"
    );
}
