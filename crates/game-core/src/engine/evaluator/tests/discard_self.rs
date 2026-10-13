use super::*;
use crate::state::{DiscardPile, Zone};

/// The Study, where [`STUDY_ATTACHMENT`] sits.
const STUDY: LocationId = LocationId(3);
/// A location attachment, which a seat cannot place, so it carries one named
/// id.
const STUDY_ATTACHMENT: CardInstanceId = CardInstanceId(9);

#[test]
fn discard_self_removes_threat_area_instance_to_encounter_discard() {
    // An encounter card the unit registry does not know, so the seat files
    // it as the encounter deck's.
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).threat_area(["01165"]))
        .build();
    let inst = state.in_play(InvestigatorId(1), "01165");
    let mut events = Vec::new();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let c =
            EvalContext::for_controller_with_source(InvestigatorId(1), AbilitySource::InPlay(inst));
        run(&mut cx, &Effect::DiscardSelf, c)
    };
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(state.investigators[&InvestigatorId(1)]
        .threat_area
        .is_empty());
    assert_eq!(state.encounter_discard, vec![CardCode::new("01165")]);
    assert_event!(
        events,
        Event::CardDiscarded { from: Zone::ThreatArea, to: DiscardPile::Encounter, code } if code.as_str() == "01165"
    );
}

#[test]
fn discard_self_removes_location_attachment_to_encounter_discard() {
    let mut loc = test_support::test_location(STUDY.0, "Study");
    loc.attachments.push(CardInPlay::enter_play(
        CardCode::new("01168"),
        STUDY_ATTACHMENT,
        Owner::EncounterDeck,
    ));
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1))
        .with_location(loc)
        .build();
    let mut events = Vec::new();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let c = EvalContext::for_controller_with_source(
            InvestigatorId(1),
            AbilitySource::InPlay(STUDY_ATTACHMENT),
        );
        run(&mut cx, &Effect::DiscardSelf, c)
    };
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(state.locations[&STUDY].attachments.is_empty());
    assert_eq!(state.encounter_discard, vec![CardCode::new("01168")]);
    assert_event!(
        events,
        Event::CardDiscarded { from: Zone::LocationAttachment, to: DiscardPile::Encounter, code } if code.as_str() == "01168"
    );
}

#[test]
fn discard_self_rejects_without_source() {
    let mut state = GameStateBuilder::new().with_seat(seat(1)).build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = run(
        &mut cx,
        &Effect::DiscardSelf,
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
}
