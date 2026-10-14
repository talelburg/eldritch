//! #482 regression: advancing The Gathering's agenda 01105 via the real Mythos
//! doom-to-threshold cascade. Its Forced reverse is the lead's interactive
//! `ChooseOne`, which suspends. The cascade must let it resolve before the 1.4
//! draws — no stranded `Effect` frame / `anchor_on_child_pop` panic.

use game_core::action::RosterEntry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{InputKind, OptionTarget};
use game_core::state::{CardCode, InvestigatorId};
use game_core::test_support::TestSession;
use scenarios::the_gathering;

const INV: InvestigatorId = InvestigatorId(1);

/// Seat Roland, set agenda doom to threshold-1, give the lead a card (so the
/// random-discard branch is legal and the `ChooseOne` genuinely suspends), and
/// `interactive_acknowledge` per the arg. Returns the session right after
/// `EndTurn`.
fn drive_to_mythos_advance(interactive: bool) -> TestSession {
    let roster = vec![RosterEntry {
        investigator: CardCode::new("01001"),
        deck: vec![],
    }];
    // seat_and_open rests at the mulligan; Roland keeps his (empty) hand.
    let mut state = TestSession::new(
        game_core::engine::seat_and_open(
            the_gathering::setup(),
            &roster,
            scenarios::TEST_REGISTRIES,
        )
        .state,
        scenarios::TEST_REGISTRIES,
    )
    .choose_from_hand(&[])
    .finish()
    .state;
    let threshold = state.agenda_deck[state.agenda_index].doom_threshold;
    state.agenda_doom = threshold - 1;
    state.encounter_discard.clear();
    state.interactive_acknowledge = interactive;
    state
        .investigators
        .get_mut(&INV)
        .unwrap()
        .hand
        .push(CardCode::new("01088"));
    TestSession::new(state, scenarios::TEST_REGISTRIES).take(&TurnAction::EndTurn)
}

#[test]
fn mythos_agenda_advance_choose_one_resolves_without_panic() {
    // Flag off: no acknowledge — the live prompt is the lead's ChooseOne.
    let session = drive_to_mythos_advance(false);
    let request = session.prompt();
    assert_eq!(
        request.kind,
        InputKind::PickSingle,
        "the lead's choose-one: {request:?}"
    );
    assert!(request.options.len() >= 2, "two branches: {request:?}");
    // The choice is live BEFORE the encounter draws, and BEFORE the cursor bumps
    // (Finalize runs only after the reverse resolves — RR order).
    assert_eq!(
        session.state().current_encounter_drawer(),
        None,
        "draws wait for the advance choice"
    );
    assert_eq!(
        session.state().agenda_index,
        0,
        "cursor bumps only after the reverse resolves"
    );

    // Resolve the choice (the second printed branch = lead takes 2 horror): the
    // agenda finalizes (cursor bumps) and the cascade proceeds into the 1.4
    // encounter draw.
    let session = session.pick_nth(1);
    assert_eq!(
        session.state().agenda_index,
        1,
        "agenda finalized after the choice"
    );
    assert_eq!(
        session.state().current_encounter_drawer(),
        Some(INV),
        "the 1.4 draws run after the advance fully resolved"
    );
}

#[test]
fn mythos_agenda_advance_acknowledge_precedes_the_choice() {
    // Flag on (server path): the forced agenda advance surfaces as an on-card
    // flip pick anchored to the agenda, and it precedes the ChooseOne (#558).
    let session = drive_to_mythos_advance(true);
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::PickSingle, "{request:?}");
    assert_eq!(request.options.len(), 1, "a single 'Advance' option");
    assert_eq!(
        request.options[0].target,
        Some(OptionTarget::Agenda),
        "the flip pick anchors to the agenda card"
    );
    // Click the flip → the ChooseOne becomes the live prompt.
    let session = session.pick(OptionTarget::Agenda);
    assert_eq!(session.prompt().kind, InputKind::PickSingle);
}
