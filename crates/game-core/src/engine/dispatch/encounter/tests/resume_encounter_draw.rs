use super::*;
use crate::engine::dispatch;
use crate::engine::outcome::InputKind;
use crate::state::{CardCode, Continuation, GameStateBuilder, InvestigatorId, Phase};
use crate::test_support;

// The former `rejects_outside_mythos_phase` / `rejects_when_no_draw_pending`
// / `rejects_when_out_of_order` tests are gone (#348 part 2c-iii-b): the
// dedicated `DrawEncounterCard` action is removed. "Outside Mythos" and "no
// draw pending" are now structurally impossible — `resume_encounter_draw` is
// only reached when an `EncounterDraw` frame is on top, which `mythos_phase`
// only pushes during Mythos — and "out of order" is gone because the folded
// `Confirm` carries no investigator (the drawer is always `remaining[0]`).
// The frame-presence gate is exercised by `apply`'s `EncounterDraw` guard
// and `resolve_input`'s no-frame rejection.

#[test]
fn rejects_non_confirm_response_and_preserves_frame() {
    // Validate-first: a non-`Confirm` response rejects and leaves the
    // `EncounterDraw` frame intact for retry.
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
        .build();
    let mut events = Vec::new();
    let outcome = resume_encounter_draw(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &InputResponse::Skip,
    );
    assert!(matches!(
        outcome,
        EngineOutcome::Rejected { reason } if reason.contains("expects InputResponse::Confirm")
    ));
    assert!(
        matches!(
            state.continuations.last(),
            Some(Continuation::EncounterDraw { remaining, .. }) if remaining == &[InvestigatorId(1)]
        ),
        "the EncounterDraw frame must survive a rejected response for retry",
    );
    assert!(events.is_empty(), "a rejected response emits no events");
}

#[test]
fn the_draw_prompt_is_anchored_to_the_encounter_deck() {
    // The prompt anchor is the only thing distinguishing this prompt from the
    // cosmetic skill-test acknowledge on the wire — both are option-less
    // `Confirm`s (ADR 0011). A rewritten builder that drops the `.at(…)`
    // relocates the Draw button to the banner, and this is what catches it.
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
        .build();
    let mut events = Vec::new();
    let outcome = prompt_encounter_draw(&Cx {
        state: &mut state,
        events: &mut events,
    });
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("the encounter draw suspends for a Confirm");
    };
    assert_eq!(request.kind, InputKind::Confirm);
    assert!(request.options.is_empty(), "a Confirm carries no options");
    assert_eq!(request.target, Some(OptionTarget::EncounterDeck));
}

/// Exercises the early-reject guard for the registry / unknown-card
/// checks. Depending on which tests have run in this process:
///
/// - `"no card registry installed"` — if no registry has been
///   installed yet in this process.
/// - `"unknown card code: ..."` — if another test has installed a
///   registry that doesn't know the synthetic code `"__no_such_card"`.
///
/// In both cases the invariant is identical: state is not further
/// mutated, the card remains in the encounter deck (the draw was
/// either blocked before or after the draw). The deck-length
/// assertion allows for the draw-then-reject case (deck shrinks by
/// at most one) matching the `encounter_card_revealed_tests` pattern.
#[test]
fn rejects_when_registry_not_installed_or_unknown_code() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1)])
        .with_mythos_draw_remaining([InvestigatorId(1)])
        .build();
    // Seed the encounter deck with an unknown code so we prove the
    // reject fires at the registry or unknown-code check, not at the
    // empty-deck check.
    state
        .encounter_deck
        .push_back(CardCode("__no_such_card".into()));
    let pre_deck_len = state.encounter_deck.len();
    let mut events = Vec::new();
    // `resume_encounter_draw` now only pushes the per-drawer `PlayerDraw`
    // chain frame; the actual draw (and its registry/unknown-code reject)
    // happens in the `drive` loop's `PlayerDraw` arm (callsite-migration).
    // Run it through `drive` so the reject surfaces as the engine produces it.
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = resume_encounter_draw(&mut cx, &InputResponse::Confirm);
        dispatch::drive(&mut cx, outcome)
    };
    match outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("no card registry installed")
                    || reason.contains("unknown card code"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    // Deck must not grow; may shrink by 1 if draw happened before
    // the unknown-code reject (documented exception matching the
    // encounter_card_revealed validate-first caveat).
    assert!(
        state.encounter_deck.len() <= pre_deck_len,
        "deck should not grow; expected <= {pre_deck_len}, got {}",
        state.encounter_deck.len(),
    );
}
