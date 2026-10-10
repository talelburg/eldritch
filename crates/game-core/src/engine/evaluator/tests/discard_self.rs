use super::*;
use crate::state::{DiscardPile, Zone};

#[test]
fn discard_self_removes_threat_area_instance_to_encounter_discard() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let inst = CardInstanceId(5);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .threat_area
        .push(CardInPlay::enter_play(
            CardCode::new("01165"),
            inst,
            Owner::EncounterDeck,
        ));
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
    assert!(events.iter().any(|e| matches!(
        e,
        Event::CardDiscarded { from: Zone::ThreatArea, to: DiscardPile::Encounter, code } if code.as_str() == "01165"
    )));
}

#[test]
fn discard_self_removes_location_attachment_to_encounter_discard() {
    let mut loc = test_support::test_location(3, "Study");
    loc.attachments.push(CardInPlay::enter_play(
        CardCode::new("01168"),
        CardInstanceId(9),
        Owner::EncounterDeck,
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
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
            AbilitySource::InPlay(CardInstanceId(9)),
        );
        run(&mut cx, &Effect::DiscardSelf, c)
    };
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(state.locations[&LocationId(3)].attachments.is_empty());
    assert_eq!(state.encounter_discard, vec![CardCode::new("01168")]);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::CardDiscarded { from: Zone::LocationAttachment, to: DiscardPile::Encounter, code } if code.as_str() == "01168"
    )));
}

#[test]
fn discard_self_rejects_without_source() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
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
