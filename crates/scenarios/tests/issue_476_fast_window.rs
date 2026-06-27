//! #476 regression: a framework Fast window no longer strands. Real registries,
//! Roland (01001), Rotting Remains (01163) on the encounter deck, a forced
//! Cultist bag, and Magnifying Glass (01030, a Fast asset) in hand. After the
//! Mythos draw + failed willpower test resolve, the `InvestigatorTurnBegins` Fast
//! window finds a fast play eligible and surfaces a SKIPPABLE `PickSingle` (not a
//! `Done`-idle strand). Skipping reaches the open turn; picking the option plays
//! the asset.

use game_core::action::RosterEntry;
use game_core::engine::{apply, seat_and_open, EngineOutcome};
use game_core::state::{CardCode, ChaosToken, GameState, InvestigatorId, Phase};
use game_core::test_support::take_turn_action;
use game_core::{Action, InputKind, InputResponse, OptionId, PlayerAction, TurnAction};
use scenarios::{the_gathering, REGISTRY};

#[ctor::ctor(unsafe)]
fn install_registries() {
    let _ = game_core::scenario_registry::install(REGISTRY);
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Drive to the post-Mythos-draw fast window: Roland holds Magnifying Glass,
/// fails the Rotting Remains willpower test (Cultist -1), and the
/// `InvestigatorTurnBegins` fast window prompts. Returns the state + outcome there.
fn to_fast_window() -> (GameState, EngineOutcome) {
    let roster = vec![RosterEntry {
        investigator: CardCode("01001".into()),
        deck: vec![],
    }];
    let mut state = seat_and_open(the_gathering::setup(), &roster).state;
    state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;
    state.encounter_deck = vec![CardCode("01163".into())].into();
    state.encounter_discard.clear();
    state.chaos_bag.tokens = vec![ChaosToken::Cultist];
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .hand
        .push(CardCode("01030".into()));

    let state = take_turn_action(state, &TurnAction::EndTurn).state;
    let state = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    )
    .state;
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    (r.state, r.outcome)
}

#[test]
fn parked_fast_window_prompts_instead_of_stranding() {
    let (state, outcome) = to_fast_window();
    let EngineOutcome::AwaitingInput { request, .. } = &outcome else {
        panic!("expected a skippable fast-window prompt, got {outcome:?} (strand regression)");
    };
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert!(
        request.skippable,
        "the fast window must be skippable (pass): {request:?}"
    );
    assert!(
        !request.options.is_empty(),
        "the fast window lists the eligible fast plays: {request:?}"
    );
    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(state.round, 2);
}

#[test]
fn skipping_the_fast_window_reaches_the_open_turn() {
    let (state, _) = to_fast_window();
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );
    let EngineOutcome::AwaitingInput { request, .. } = &r.outcome else {
        panic!("Skip must reach the open turn, got {:?}", r.outcome);
    };
    assert_eq!(request.prompt, "Choose an action");
}

#[test]
fn playing_the_fast_card_puts_magnifying_glass_in_play() {
    let (state, _) = to_fast_window();
    // Option 0 is the only eligible fast play (Magnifying Glass).
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert!(
        r.state.investigators[&InvestigatorId(1)]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode("01030".into())),
        "Magnifying Glass entered play after the fast-window pick"
    );
}
