//! Integration (#556): an agenda's forced-on-advance acknowledge anchors to the
//! agenda card on the board, not the flat prompt bar. Own process → installs the
//! real `cards::REGISTRY`.
//!
//! Drives What's Going On?! (01105)'s `AgendaAdvanced` forced with
//! `interactive_acknowledge` on, so the one-option "Resolve" acknowledge surfaces
//! *before* the effect (the #466 confirm-before-effect pause) — and asserts its
//! anchor is `OptionTarget::Agenda`. The subsequent discard-vs-horror
//! `ChooseOne` is a separate evaluator prompt, and since #775 closed #555 it
//! anchors to the agenda too; the second test below pins that, because an
//! un-anchored option is silently rendered in the banner instead.

use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::{EngineOutcome, OptionId, OptionTarget};
use game_core::state::{Agenda, CardCode, GameState, InvestigatorId};
use game_core::test_support::{fire_forced_on_agenda_advance, test_investigator, GameStateBuilder};

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

fn state_on_agenda_01105() -> GameState {
    let lead = InvestigatorId(1);
    // A real investigator code so any registry-backed lookup resolves; Skids
    // O'Toole (01003) has no implemented abilities (mirrors agenda_reverses.rs).
    let mut inv = test_investigator(1);
    inv.investigator_card.code = CardCode::new("01003");
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([lead])
        .build();
    // The current agenda must be in the deck for the scan to reach 01105's
    // ability at all; the candidate it mints names `AbilitySource::Agenda`, and
    // that is what the ack anchors to (#735).
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    state.interactive_acknowledge = true;
    state
}

#[test]
fn agenda_01105_forced_ack_anchors_to_the_agenda_card() {
    let mut state = state_on_agenda_01105();
    let mut events = Vec::new();
    let out = fire_forced_on_agenda_advance(&mut state, &mut events, CardCode::new("01105"));
    match out {
        EngineOutcome::AwaitingInput { request, .. } => {
            assert_eq!(
                request.options.len(),
                1,
                "the interactive forced-acknowledge is a one-option 'Resolve' pick \
                 before the effect resolves",
            );
            assert_eq!(
                request.options[0].target,
                Some(OptionTarget::Agenda),
                "an agenda forced-on-advance ack anchors to the agenda card (#556)",
            );
        }
        other => panic!("expected the forced-acknowledge suspend, got {other:?}"),
    }
}

/// 01105's printed *"choose one"* renders on the agenda card, under the two
/// labels split from its printed sentence — not in the prompt banner under the
/// branches' `Debug` form, which is what shipped until #775.
#[test]
fn agenda_01105_choose_one_anchors_to_the_agenda_card_under_its_printed_labels() {
    let mut state = state_on_agenda_01105();
    let mut events = Vec::new();
    let out = fire_forced_on_agenda_advance(&mut state, &mut events, CardCode::new("01105"));
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "the interactive forced-ack comes first: {out:?}",
    );
    // Acknowledge, so the effect — and its ChooseOne — resolves.
    let resumed = game_core::engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    let EngineOutcome::AwaitingInput { request, .. } = &resumed.outcome else {
        panic!("expected 01105's ChooseOne, got {:?}", resumed.outcome);
    };
    assert_eq!(
        request
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        [
            "Each investigator discards 1 card at random from his or her hand",
            "The lead investigator takes 2 horror",
        ],
    );
    for option in &request.options {
        assert_eq!(option.target, Some(OptionTarget::Agenda), "{request:?}");
    }
}
