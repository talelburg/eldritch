use super::*;
use crate::engine::outcome::InputKind;
use crate::state::{
    GameState, GameStateBuilder, InvestigatorId, MythosPhaseFrame, MythosResume, Phase,
};
use crate::test_support::{self, TestSession, TEST_REGISTRIES};

// `resume_encounter_draw` has no out-of-Mythos, no-draw-pending or
// out-of-order case to reject. It is reached only when an `EncounterDraw`
// frame is on top, which `mythos_phase` pushes only during Mythos, and the
// `Confirm` it resumes on carries no investigator (the drawer is always
// `remaining[0]`). The frame-presence gate is exercised by `apply`'s
// `EncounterDraw` guard and `resolve_input`'s no-frame rejection.

/// Investigator 1's Mythos step-1.4 encounter draw, the moment it is asked.
fn drawing() -> GameState {
    drawing_board().build()
}

/// [`drawing`], before it is built.
fn drawing_board() -> GameStateBuilder {
    GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_seating([InvestigatorId(1)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
}

#[test]
fn rejects_non_confirm_response_and_preserves_frame() {
    // Validate-first: a non-`Confirm` response rejects and leaves the
    // encounter draw in place for retry.
    let session = TestSession::new(drawing(), TEST_REGISTRIES);
    let events_before = session.events().len();
    let session = session.skip();

    assert!(
        session
            .expect_rejected()
            .contains("expects InputResponse::Confirm"),
        "unexpected reject reason: {:?}",
        session.expect_rejected(),
    );
    // The encounter draw must survive a rejected response for retry.
    session.assert_at_encounter_draw();
    assert_eq!(
        session.state().current_encounter_drawer(),
        Some(InvestigatorId(1))
    );
    assert_eq!(
        session.events().len(),
        events_before,
        "a rejected response emits no events"
    );
}

#[test]
fn the_draw_prompt_is_anchored_to_the_encounter_deck() {
    // The prompt anchor is the only thing distinguishing this prompt from the
    // cosmetic skill-test acknowledge on the wire — both are option-less
    // `Confirm`s (ADR 0011). A rewritten builder that drops the `.at(…)`
    // relocates the Draw button to the banner, and this is what catches it.
    let session = TestSession::new(drawing(), TEST_REGISTRIES);
    let request = session.prompt();
    assert_eq!(request.kind, InputKind::Confirm);
    assert!(request.options.is_empty(), "a Confirm carries no options");
    assert_eq!(request.target, Some(OptionTarget::EncounterDeck));
}

/// Exercises the early-reject guard for the unknown-card check: the
/// test registry doesn't know the synthetic code
/// `"__no_such_card"`, so the draw rejects with `"unknown card code: ..."`.
///
/// State is not further mutated. The deck-length assertion allows for the
/// draw-then-reject case (deck shrinks by at most one) matching the
/// `encounter_card_revealed_tests` pattern.
#[test]
fn rejects_an_unknown_encounter_code() {
    // Seed the encounter deck with an unknown code so we prove the
    // reject fires at the unknown-code check, not at the
    // empty-deck check.
    let state = drawing_board()
        .with_encounter_deck(["__no_such_card"])
        .build();
    let pre_deck_len = state.encounter_deck.len();
    // `resume_encounter_draw` only pushes the per-drawer `PlayerDraw` chain
    // frame; the actual draw (and its unknown-code reject) happens in the
    // `drive` loop's `PlayerDraw` arm, which the step's drain runs.
    let session = TestSession::new(state, TEST_REGISTRIES).confirm();
    let reason = session.expect_rejected();
    assert!(
        reason.contains("unknown card code"),
        "unexpected reject reason: {reason:?}",
    );
    // Deck must not grow; may shrink by 1 if draw happened before
    // the unknown-code reject (documented exception matching the
    // encounter_card_revealed validate-first caveat).
    assert!(
        session.state().encounter_deck.len() <= pre_deck_len,
        "deck should not grow; expected <= {pre_deck_len}, got {}",
        session.state().encounter_deck.len(),
    );
}
