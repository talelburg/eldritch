use super::*;
use crate::engine::outcome::InputKind;
use crate::state::{Continuation, GameStateBuilder, InvestigatorId, Phase};
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
