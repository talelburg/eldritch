use super::*;

/// Every defeat cause lands on the same status; only the event's cause
/// separates them (#814).
///
/// `glossary/Campaign_Play.md`: *"If an investigator is defeated by taking
/// damage equal to his or her health, he or she suffers 1 physical trauma
/// (recorded in the campaign log). … If an investigator has physical trauma
/// equal to his or her printed health, the investigator is killed."* So a
/// defeat by damage at zero prior trauma leaves the investigator
/// **defeated**, owing one trauma — not killed. The engine used to assert
/// the kill here; the derivation belongs to the campaign log (#766), which
/// reads the recorded totals.
#[test]
fn damage_and_horror_defeats_both_land_on_defeated() {
    for cause in [EliminationCause::Damage, EliminationCause::Horror] {
        let a = InvestigatorId(1);
        let mut state = two_investigator_open_turn(a);
        let mut events = Vec::new();

        apply_investigator_elimination(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            a,
            cause,
        );

        assert_eq!(
            state.investigators[&a].status,
            Status::Defeated,
            "{cause:?} defeat is a defeat, not a kill or an insanity",
        );
        assert_event!(events, Event::InvestigatorEliminated { investigator, cause: c }
            if *investigator == a && *c == cause);
    }
}

/// `glossary/Defeat.md`: *"An investigator might also be defeated by a card
/// ability."* It lands on the one defeat status, and the cause it carries on
/// the event is what distinguishes it from a damage or horror defeat.
#[test]
fn a_card_ability_defeat_lands_on_defeated_and_carries_its_cause() {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut state = two_investigator_open_turn(a);
    let mut events = Vec::new();

    defeat_investigator(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        a,
    );

    assert_eq!(
        state.investigators[&a].status,
        Status::Defeated,
        "the one defeat status; the cause rides the event",
    );
    assert_event!(events, Event::InvestigatorEliminated { investigator, cause }
        if *investigator == a && *cause == EliminationCause::CardAbility);
    assert_eq!(
        state.investigators[&b].status,
        Status::Active,
        "the other investigator is untouched",
    );
    assert!(
        state.ending.is_none(),
        "one active investigator remains, so the scenario has not ended",
    );
}

/// Elimination step 6, *"If there are no remaining players, the scenario
/// ends"* — reached through the ordinary defeat path, so a card ability that
/// drains the last active investigator ends the scenario at **no** resolution
/// point without latching one itself.
#[test]
fn a_card_ability_defeating_the_last_investigator_latches_no_resolution() {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut state = two_investigator_open_turn(a);
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    defeat_investigator(&mut cx, a);
    defeat_investigator(&mut cx, b);

    assert_event!(events, Event::AllInvestigatorsEliminated);
    assert_eq!(
        state.ending,
        Some(ScenarioEnding::NoResolution),
        "Elimination step 6, not a numbered resolution",
    );
}

/// An investigator who is not `Active` is not defeated again — which is what
/// lets a card printing *"each investigator that has not resigned"* skip the
/// filter entirely.
#[test]
fn a_card_ability_defeat_no_ops_on_an_already_eliminated_investigator() {
    let (a, b) = (InvestigatorId(1), InvestigatorId(2));
    let mut state = two_investigator_open_turn(b);
    state.investigators.get_mut(&a).expect("seated").status = Status::Resigned;
    let mut events = Vec::new();

    defeat_investigator(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        a,
    );

    assert_eq!(
        state.investigators[&a].status,
        Status::Resigned,
        "the resigned investigator is left alone",
    );
    assert_no_event!(events, Event::InvestigatorEliminated { .. });
}
