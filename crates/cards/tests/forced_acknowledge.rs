//! #466: a no-choice forced location ability (the Attic's 1 horror, the Cellar's
//! 1 damage) surfaces a one-option acknowledge *before* the harm lands when
//! `interactive_acknowledge` is on, and resolves synchronously when it is off.
//!
//! Own process → installs `cards::REGISTRY`. The forced effect is driven through
//! the real `EnteredLocation` timing point via `TestSession::fire_at`; the
//! interactive pause is then resumed by picking the acknowledge (the same way a
//! host resumes an `AwaitingInput`).

use cards::REGISTRY;
use game_core::engine::{ApplyResult, EngineOutcome, OptionTarget, TimingEvent};
use game_core::state::{CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, TestSession};

const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(1);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// One investigator standing on a location whose card `code` carries a forced
/// on-enter ability, with `interactive_acknowledge` set as given.
fn state_on_location(code: &str, interactive: bool) -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LOC);
    let mut loc = test_support::test_location(1, "Forced Location");
    loc.code = CardCode::new(code);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc)
        .with_turn_order([INV])
        .build();
    state.interactive_acknowledge = interactive;
    state
}

/// Fire `INV` entering `LOC` on the location `code`.
fn enter(code: &str, interactive: bool) -> TestSession {
    TestSession::new(state_on_location(code, interactive)).fire_at(TimingEvent::EnteredLocation {
        investigator: INV,
        location: LOC,
    })
}

/// The acknowledge is a one-option pick anchored to the location on the map
/// (#553), not the flat bar.
fn assert_acknowledge_at_the_location(session: &TestSession) {
    let request = session.prompt();
    assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
    assert_eq!(
        request.options[0].target,
        Some(OptionTarget::Location(LOC)),
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

    let ApplyResult { state, outcome, .. } = paused.pick(OptionTarget::Location(LOC)).finish();
    assert_eq!(
        outcome,
        EngineOutcome::Done,
        "nothing is left pending after the acknowledge"
    );
    assert_eq!(
        state.investigators[&INV].horror(),
        1,
        "horror applied after the acknowledge"
    );
}

#[test]
fn attic_forced_resolves_synchronously_when_not_interactive() {
    let ApplyResult { state, outcome, .. } = enter("01113", false).finish();
    assert!(
        matches!(outcome, EngineOutcome::Done),
        "flag off: no suspend, got {outcome:?}"
    );
    assert_eq!(
        state.investigators[&INV].horror(),
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

    let ApplyResult { state, outcome, .. } = paused.pick(OptionTarget::Location(LOC)).finish();
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&INV].damage(),
        1,
        "damage applied after the acknowledge"
    );
}

#[test]
fn cellar_forced_resolves_synchronously_when_not_interactive() {
    let ApplyResult { state, outcome, .. } = enter("01114", false).finish();
    assert!(
        matches!(outcome, EngineOutcome::Done),
        "flag off: no suspend, got {outcome:?}"
    );
    assert_eq!(
        state.investigators[&INV].damage(),
        1,
        "flag off: damage applied synchronously (today's behavior)"
    );
}
