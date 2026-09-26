use super::*;

#[test]
fn seat_and_open_opens_mulligan_for_a_synthetic_roster() {
    test_support::install_test_registry();

    let setup = GameStateBuilder::new().build(); // round 0, no investigators
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: vec![],
    }];

    let result = seat_and_open(setup, &roster);

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        result.outcome
    );
    assert_eq!(result.state.round, 1);
    assert!(result.state.investigators.contains_key(&InvestigatorId(1)));
}

#[test]
fn seat_and_open_rejects_an_unknown_investigator_code() {
    test_support::install_test_registry();

    let setup = GameStateBuilder::new().build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new("99999"),
        deck: vec![],
    }];

    let result = seat_and_open(setup, &roster);

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        result.state.round, 0,
        "rejected seating leaves state unchanged"
    );
}
