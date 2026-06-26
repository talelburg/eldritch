//! #482 regression: advancing The Gathering's agenda 01105 via the real Mythos
//! doom-to-threshold cascade. Its Forced reverse is the lead's interactive
//! ChooseOne, which suspends. The cascade must let it resolve before the 1.4
//! draws — no stranded Effect frame / anchor_on_child_pop panic.

use game_core::action::RosterEntry;
use game_core::engine::{seat_and_open, ApplyResult, EngineOutcome};
use game_core::state::{CardCode, GameState};
use game_core::test_support::take_turn_action;
use game_core::{apply, Action, InputKind, InputResponse, OptionId, PlayerAction, TurnAction};
use scenarios::{the_gathering, REGISTRY};

#[ctor::ctor]
fn install_registries() {
    let _ = game_core::scenario_registry::install(REGISTRY);
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Seat Roland, set agenda doom to threshold-1, give the lead a card (so the
/// random-discard branch is legal and the ChooseOne genuinely suspends), and
/// `interactive_acknowledge` per the arg. Returns the result right after EndTurn.
fn drive_to_mythos_advance(interactive: bool) -> ApplyResult {
    let roster = vec![RosterEntry {
        investigator: CardCode("01001".into()),
        deck: vec![],
    }];
    let mut state: GameState = seat_and_open(the_gathering::setup(), &roster).state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    let threshold = state.agenda_deck[state.agenda_index].doom_threshold;
    state.agenda_doom = threshold - 1;
    state.encounter_discard.clear();
    state.interactive_acknowledge = interactive;
    state
        .investigators
        .values_mut()
        .next()
        .unwrap()
        .hand
        .push(CardCode("01088".into()));
    take_turn_action(state, &TurnAction::EndTurn)
}

#[test]
fn mythos_agenda_advance_choose_one_resolves_without_panic() {
    // Flag off: no acknowledge — the live prompt is the lead's ChooseOne.
    let r = drive_to_mythos_advance(false);
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!("expected the agenda ChooseOne prompt, got {:?}", r.outcome);
    };
    assert_eq!(
        request.kind,
        InputKind::PickSingle,
        "the lead's choose-one: {request:?}"
    );
    assert!(request.options.len() >= 2, "two branches: {request:?}");
    // The choice is live BEFORE the encounter draws, and BEFORE the cursor bumps
    // (Finalize runs only after the reverse resolves — RR order).
    assert_eq!(
        r.state.current_encounter_drawer(),
        None,
        "draws wait for the advance choice"
    );
    assert_eq!(
        r.state.agenda_index, 0,
        "cursor bumps only after the reverse resolves"
    );

    // Resolve the choice (branch 1 = lead takes 2 horror): the agenda finalizes
    // (cursor bumps) and the cascade proceeds into the 1.4 encounter draw.
    let r2 = apply(
        r.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }),
    );
    assert_eq!(
        r2.state.agenda_index, 1,
        "agenda finalized after the choice"
    );
    assert_eq!(
        r2.state.current_encounter_drawer(),
        Some(game_core::state::InvestigatorId(1)),
        "the 1.4 draws run after the advance fully resolved"
    );
}

#[test]
fn mythos_agenda_advance_acknowledge_precedes_the_choice() {
    // Flag on (server path): the acknowledge Confirm precedes the ChooseOne.
    let r = drive_to_mythos_advance(true);
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!(
            "expected the advance acknowledge Confirm, got {:?}",
            r.outcome
        );
    };
    assert_eq!(request.kind, InputKind::Confirm, "{request:?}");
    // Acknowledge → the ChooseOne becomes the live prompt.
    let r2 = apply(
        r.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    let EngineOutcome::AwaitingInput { request, .. } = &r2.outcome else {
        panic!(
            "expected the ChooseOne after acknowledge, got {:?}",
            r2.outcome
        );
    };
    assert_eq!(request.kind, InputKind::PickSingle);
}
