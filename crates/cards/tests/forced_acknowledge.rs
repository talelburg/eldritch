//! #466: a no-choice forced location ability (the Attic's 1 horror, the Cellar's
//! 1 damage) surfaces a one-option acknowledge *before* the harm lands when
//! `interactive_acknowledge` is on, and resolves synchronously when it is off.
//!
//! Own process → installs `cards::REGISTRY`. The forced effect is driven through
//! the real `EnteredLocation` dispatch via `fire_forced_on_enter`; the
//! interactive pause is then resumed through the public `apply(ResolveInput)`
//! path (the same way a host resumes an `AwaitingInput`).

use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::state::{CardCode, Continuation, GameState, InvestigatorId, LocationId};
use game_core::test_support::{
    fire_forced_on_enter, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, InputResponse, OptionId, PlayerAction};

const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(1);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// One investigator standing on a location whose card `code` carries a forced
/// on-enter ability, with `interactive_acknowledge` set as given.
fn state_on_location(code: &str, interactive: bool) -> GameState {
    let mut inv = test_investigator(1);
    inv.current_location = Some(LOC);
    // Back the investigator with a real corpus card so the harm path's defeat
    // check (max_health/max_sanity, read from the registry) resolves — Roland
    // Banks (01001), 9 health / 5 sanity, so 1 harm never defeats.
    inv.investigator_card.code = CardCode::new("01001");
    let mut loc = test_location(1, "Forced Location");
    loc.code = CardCode::new(code);
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc)
        .with_turn_order([INV])
        .build();
    state.interactive_acknowledge = interactive;
    state
}

fn resume_single_option(state: GameState) -> game_core::engine::ApplyResult {
    game_core::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    )
}

#[test]
fn attic_forced_acknowledges_before_horror_when_interactive() {
    let mut state = state_on_location("01113", true); // the Attic — 1 horror
    let mut events = Vec::new();
    let out = fire_forced_on_enter(&mut state, &mut events, INV, LOC);
    match out {
        EngineOutcome::AwaitingInput { request, .. } => {
            assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
            assert_eq!(
                request.options[0].target,
                OptionTarget::Location(LOC),
                "the forced-on-enter option anchors to the location on the map (#553), not the flat bar"
            );
        }
        other => panic!("expected a one-option acknowledge, got {other:?}"),
    }
    assert_eq!(
        state.investigators[&INV].horror(),
        0,
        "horror must not be applied before the player acknowledges"
    );

    let result = resume_single_option(state);
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state.investigators[&INV].horror(),
        1,
        "horror applied after the acknowledge"
    );
    assert!(
        !result
            .state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::AcknowledgeForced { .. })),
        "the acknowledge frame must be gone after resume"
    );
}

#[test]
fn attic_forced_resolves_synchronously_when_not_interactive() {
    let mut state = state_on_location("01113", false);
    let mut events = Vec::new();
    let out = fire_forced_on_enter(&mut state, &mut events, INV, LOC);
    assert!(
        matches!(out, EngineOutcome::Done),
        "flag off: no suspend, got {out:?}"
    );
    assert_eq!(
        state.investigators[&INV].horror(),
        1,
        "flag off: horror applied synchronously (today's behavior)"
    );
}

#[test]
fn cellar_forced_acknowledges_before_damage_when_interactive() {
    let mut state = state_on_location("01114", true); // the Cellar — 1 damage
    let mut events = Vec::new();
    let out = fire_forced_on_enter(&mut state, &mut events, INV, LOC);
    match out {
        EngineOutcome::AwaitingInput { request, .. } => {
            assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
            assert_eq!(
                request.options[0].target,
                OptionTarget::Location(LOC),
                "the forced-on-enter option anchors to the location on the map (#553)"
            );
        }
        other => panic!("expected a one-option acknowledge, got {other:?}"),
    }
    assert_eq!(
        state.investigators[&INV].damage(),
        0,
        "damage must not be applied before the player acknowledges"
    );

    let result = resume_single_option(state);
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state.investigators[&INV].damage(),
        1,
        "damage applied after the acknowledge"
    );
}

#[test]
fn cellar_forced_resolves_synchronously_when_not_interactive() {
    let mut state = state_on_location("01114", false);
    let mut events = Vec::new();
    let out = fire_forced_on_enter(&mut state, &mut events, INV, LOC);
    assert!(
        matches!(out, EngineOutcome::Done),
        "flag off: no suspend, got {out:?}"
    );
    assert_eq!(
        state.investigators[&INV].damage(),
        1,
        "flag off: damage applied synchronously (today's behavior)"
    );
}
