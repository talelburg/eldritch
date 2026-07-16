//! Integration (#556): an agenda's forced-on-advance acknowledge anchors to the
//! agenda card on the board, not the flat prompt bar. Own process → installs the
//! real `cards::REGISTRY`.
//!
//! Drives What's Going On?! (01105)'s `AgendaAdvanced` forced with
//! `interactive_acknowledge` on, so the one-option "Resolve" acknowledge surfaces
//! *before* the effect (the #466 confirm-before-effect pause) — and asserts its
//! anchor is `OptionTarget::Agenda`. The subsequent discard-vs-horror `ChooseOne`
//! is a separate evaluator prompt whose non-entity branches stay `Global`
//! (tracked in #555); this test covers only the forced-ack anchor.

use game_core::engine::{EngineOutcome, OptionTarget};
use game_core::state::{Agenda, CardCode, InvestigatorId};
use game_core::test_support::{fire_forced_on_agenda_advance, test_investigator, GameStateBuilder};

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

#[test]
fn agenda_01105_forced_ack_anchors_to_the_agenda_card() {
    let lead = InvestigatorId(1);
    // A real investigator code so any registry-backed lookup resolves; Skids
    // O'Toole (01003) has no implemented abilities (mirrors agenda_reverses.rs).
    let mut inv = test_investigator(1);
    inv.investigator_card.code = CardCode::new("01003");
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([lead])
        .build();
    // The current agenda must be in the deck so `current_agenda_code` resolves to
    // it and the `Board`-sourced forced anchors to `Agenda` (not `Global`).
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
        resolution: None,
    }];
    state.agenda_index = 0;
    state.interactive_acknowledge = true;

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
                OptionTarget::Agenda,
                "an agenda forced-on-advance ack anchors to the agenda card (#556)",
            );
        }
        other => panic!("expected the forced-acknowledge suspend, got {other:?}"),
    }
}
