//! Integration (#556): an agenda's forced-on-advance acknowledge anchors to the
//! agenda card on the board, not the flat prompt bar. Runs against
//! `cards::TEST_REGISTRIES`.
//!
//! Drives What's Going On?! (01105)'s `AgendaAdvanced` forced with
//! `interactive_acknowledge` on, so the one-option "Resolve" acknowledge surfaces
//! *before* the effect (the #466 confirm-before-effect pause) — and asserts its
//! anchor is `OptionTarget::Agenda`. The subsequent discard-vs-horror
//! `ChooseOne` is a separate evaluator prompt, and since #775 closed #555 it
//! anchors to the agenda too; the second test below pins that, because an
//! un-anchored option is silently rendered in the banner instead.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, PromptNature};
use game_core::state::{Agenda, CardCode, GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, TestSession};

const LEAD: InvestigatorId = InvestigatorId(1);

/// The lead's turn on agenda 01105, one doom short of its threshold, so ending
/// the turn runs the round out and the next Mythos phase's step-1.2 doom
/// advances the agenda.
fn state_on_agenda_01105() -> GameState {
    // A filler deck card so Upkeep step 4.4's draw doesn't deal empty-deck
    // horror.
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).deck(["01088"]))
        .open_turn(LEAD)
        .build();
    // The current agenda must be in the deck for the scan to reach 01105's
    // ability at all; the candidate it mints names `AbilitySource::Agenda`, and
    // that is what the ack anchors to (#735).
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    state.agenda_doom = 2;
    state.interactive_acknowledge = true;
    state
}

/// Advance 01105 for real and acknowledge its flip (#558), so the session
/// rests at 01105's `AgendaAdvanced` forced, paused on the acknowledge.
fn advance_01105() -> TestSession {
    let flipped = TestSession::new_with_registries(state_on_agenda_01105(), TEST_REGISTRIES)
        .take(&TurnAction::EndTurn);
    assert_eq!(
        flipped.prompt().prompt,
        "Agenda 1 advanced — acknowledge.",
        "the round runs out and the Mythos doom advances the agenda",
    );
    flipped.pick(OptionTarget::Agenda)
}

#[test]
fn agenda_01105_forced_ack_anchors_to_the_agenda_card() {
    let session = advance_01105();
    let request = session.prompt();
    assert!(
        request.prompt.starts_with("Forced"),
        "past the flip, the session rests at 01105's forced: {request:?}",
    );
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

/// 01105's printed *"choose one"* renders on the agenda card, under the two
/// labels split from its printed sentence — not in the prompt banner under the
/// branches' `Debug` form, which is what shipped until #775.
#[test]
fn agenda_01105_choose_one_anchors_to_the_agenda_card_under_its_printed_labels() {
    // Acknowledge, so the effect — and its ChooseOne — resolves.
    let session = advance_01105().pick(OptionTarget::Agenda);
    let request = session.prompt();
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

/// 01105's *"The lead investigator must decide (choose one)"* is a **decision**
/// (ADR 0015): its branches are the two alternatives printed on the agenda's
/// reverse, so the client presents them the moment they arise instead of asking
/// for another click on the agenda card.
#[test]
fn agenda_01105_choose_one_is_a_decision_prompt() {
    let session = advance_01105();
    let request = session.prompt();
    assert_eq!(
        request.nature,
        PromptNature::Selection,
        "the forced acknowledge offers the agenda's ability, a board entity: {request:?}",
    );

    let session = session.pick(OptionTarget::Agenda);
    let request = session.prompt();
    assert_eq!(
        request.nature,
        PromptNature::Decision,
        "the discard-vs-horror choice is printed on one card: {request:?}",
    );
    assert_eq!(
        request.target,
        Some(OptionTarget::Agenda),
        "the decision carries its source anchor on the request too, so the modal \
         can name the card the choice came from: {request:?}",
    );
}
