use super::*;

#[test]
fn start_scenario_advances_to_investigation_with_round_one() {
    // seat_and_open opens the mulligan window; the Investigation phase
    // does NOT begin until the last mulligan completes (Rules Reference
    // p.27: no action windows during setup; the game begins after
    // mulligans). After seat_and_open alone, active_investigator is
    // None and no PhaseStarted(Investigation) fires yet.
    //
    // The full round-1 kickoff (active investigator set, PhaseStarted
    // fired) is covered by
    // `investigation_phase_tests::mulligan_completion_kicks_off_investigation_phase`.
    test_support::install_test_registry();
    let state = GameStateBuilder::new().build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: vec![],
    }];
    let start_result = seat_and_open(state, &roster);
    let id = InvestigatorId(1);

    assert!(
        matches!(start_result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt (AwaitingInput), got {:?}",
        start_result.outcome
    );
    assert_eq!(start_result.state.round, 1);
    assert_eq!(start_result.state.phase, Phase::Investigation);
    // The mulligan loop is in progress — active investigator not yet set.
    assert_eq!(
        start_result.state.current_mulligan(),
        Some(id),
        "mulligan loop must prompt the first investigator after seat_and_open"
    );
    assert_eq!(
        start_result.state.active_investigator, None,
        "active investigator is not set until the mulligan loop drains"
    );
    assert_eq!(start_result.state.investigators[&id].actions_remaining, 3);

    assert_event!(start_result.events, Event::ScenarioStarted);
    // Round 1: Mythos is skipped entirely — no PhaseStarted(Mythos) or
    // PhaseEnded(Mythos) fire (Rules Reference p.24: first round skips
    // the Mythos phase; the phase doesn't happen, not "runs empty").
    assert_no_event!(
        start_result.events,
        Event::PhaseStarted {
            phase: Phase::Mythos
        }
    );
    assert_no_event!(
        start_result.events,
        Event::PhaseEnded {
            phase: Phase::Mythos
        }
    );
    // PhaseStarted(Investigation) fires at mulligan completion, not here.
    assert_no_event!(
        start_result.events,
        Event::PhaseStarted {
            phase: Phase::Investigation
        }
    );

    // After the sole investigator mulligans (an empty "keep my hand"
    // PickMultiple), the phase begins and the lead becomes active.
    let mulligan_result = apply(
        start_result.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert!(matches!(
        mulligan_result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        mulligan_result.state.current_mulligan(),
        None,
        "mulligan loop must drain"
    );
    assert_eq!(
        mulligan_result.state.active_investigator,
        Some(id),
        "lead investigator becomes active after mulligan window closes"
    );
    assert_event!(
        mulligan_result.events,
        Event::PhaseStarted {
            phase: Phase::Investigation
        }
    );
    // rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2 / start_scenario seed);
    // actions_remaining == 3 is verified above via assert_eq.
}

#[test]
fn start_scenario_on_already_started_state_is_rejected() {
    // seat_and_open calls start_scenario which rejects when round != 0;
    // the roster check never runs, so an empty slice is enough.
    let state = GameStateBuilder::new().with_round(7).build();
    let result = seat_and_open(state, &[]);

    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(result.state.round, 7);
    assert!(result.events.is_empty());
}

#[test]
fn start_scenario_shuffles_each_deck_and_deals_initial_hand() {
    test_support::install_test_registry();
    let state = GameStateBuilder::new().with_rng_seed(42).build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: make_test_deck(10),
    }];
    let result = seat_and_open(state, &roster);
    let id = InvestigatorId(1);

    // seat_and_open deals hands, then opens the mulligan prompt
    // (AwaitingInput) — the deal happens before the prompt.
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::DeckShuffled { investigator } if *investigator == id
    );
    assert_event!(
        result.events,
        Event::CardsDrawn { investigator, count: 5 } if *investigator == id
    );
    // Hand has 5 cards, deck has 5 left, both partitions cover the
    // original 10 cards (just shuffled).
    let inv_after = &result.state.investigators[&id];
    assert_eq!(inv_after.hand.len(), 5);
    assert_eq!(inv_after.deck.len(), 5);
    let mut all: Vec<_> = inv_after.hand.iter().chain(inv_after.deck.iter()).collect();
    all.sort();
    let mut expected: Vec<_> = make_test_deck(10).into_iter().collect();
    expected.sort();
    assert_eq!(
        all.iter().map(|c| CardCode::as_str(c)).collect::<Vec<_>>(),
        expected.iter().map(CardCode::as_str).collect::<Vec<_>>()
    );
}

#[test]
fn start_scenario_with_empty_deck_yields_empty_hand_and_no_events() {
    test_support::install_test_registry();
    let state = GameStateBuilder::new().build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: vec![],
    }];
    let result = seat_and_open(state, &roster);
    let id = InvestigatorId(1);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    // Empty-deck no-op shuffle: no event.
    assert_no_event!(result.events, Event::DeckShuffled { .. });
    // draw_cards still emits CardsDrawn { count: 0 } so consumers
    // see the attempt.
    assert_event!(
        result.events,
        Event::CardsDrawn { investigator, count: 0 } if *investigator == id
    );
    assert!(result.state.investigators[&id].hand.is_empty());
    assert!(result.state.investigators[&id].deck.is_empty());
}

#[test]
fn start_scenario_with_short_deck_draws_only_what_remains() {
    // Deck of 3, INITIAL_HAND_SIZE is 5: draw 3, deck empties, no
    // panic.
    test_support::install_test_registry();
    let state = GameStateBuilder::new().with_rng_seed(7).build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: make_test_deck(3),
    }];
    let result = seat_and_open(state, &roster);
    let id = InvestigatorId(1);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::CardsDrawn { investigator, count: 3 } if *investigator == id
    );
    assert_eq!(result.state.investigators[&id].hand.len(), 3);
    assert!(result.state.investigators[&id].deck.is_empty());
}

#[test]
fn start_scenario_handles_multiple_investigators_deterministically() {
    // Three investigators seated from the roster. Roster order determines
    // id assignment (1, 2, 3 sequentially); each gets their own deck +
    // hand independently. BTreeMap iteration is sorted so shuffle order
    // is deterministic.
    test_support::install_test_registry();
    let state = GameStateBuilder::new().with_rng_seed(2026).build();
    let roster: Vec<RosterEntry> = (0..3)
        .map(|_| RosterEntry {
            investigator: CardCode::new(test_support::TEST_INV),
            deck: make_test_deck(8),
        })
        .collect();
    let result = seat_and_open(state, &roster);
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    // Each investigator drew 5 cards and has 3 left in deck.
    let ids = [InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)];
    for id in ids {
        let inv_after = &result.state.investigators[&id];
        assert_eq!(inv_after.hand.len(), 5);
        assert_eq!(inv_after.deck.len(), 3);
    }
    // Each emitted CardsDrawn { count: 5 }.
    assert_event_count!(result.events, 3, Event::CardsDrawn { count: 5, .. });
}
