use super::*;

/// Build a Draw scenario: one investigator at A, in Investigation
/// phase, active, 3 actions. The caller mutates deck/hand/discard
/// before the test.
fn draw_scenario() -> (InvestigatorId, GameState) {
    // Registry needed for max_health()/max_sanity() after cp2a.
    test_support::install_test_registry();
    let id = InvestigatorId(1);
    let a = LocationId(10);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(a);
    inv.actions_remaining = 3;
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_support::test_location(10, "A"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_rng_seed(13)
        .with_phase_anchor(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(id)
        .build();
    (id, state)
}

#[test]
fn draw_with_non_empty_deck_draws_one_and_spends_action() {
    let (id, mut state) = draw_scenario();
    state.investigators.get_mut(&id).unwrap().deck =
        vec![CardCode::new("test-001"), CardCode::new("test-002")];
    let result = test_support::take_turn_action(state, &TurnAction::Draw { investigator: id });
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == id
    );
    assert_event!(
        result.events,
        Event::CardsDrawn { investigator, count: 1 } if *investigator == id
    );
    assert_no_event!(result.events, Event::DeckShuffled { .. });
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.hand.len(), 1);
    assert_eq!(inv.deck.len(), 1);
    assert_eq!(inv.hand[0], CardCode::new("test-001"));
    assert_eq!(inv.actions_remaining, 2);
}

#[test]
fn draw_with_empty_deck_reshuffles_discard_draws_and_takes_one_horror() {
    // Per the Rules Reference: empty deck on draw triggers
    // reshuffle, then draw, then 1 horror penalty.
    let (id, mut state) = draw_scenario();
    state.investigators.get_mut(&id).unwrap().discard = vec![
        CardCode::new("test-A"),
        CardCode::new("test-B"),
        CardCode::new("test-C"),
    ];
    let result = test_support::take_turn_action(state, &TurnAction::Draw { investigator: id });
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::DeckShuffled { investigator } if *investigator == id
    );
    assert_event!(
        result.events,
        Event::CardsDrawn { investigator, count: 1 } if *investigator == id
    );
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == id
    );
    let shuffle_idx = result
        .events
        .iter()
        .position(|e| matches!(e, Event::DeckShuffled { .. }))
        .expect("DeckShuffled missing");
    let draw_idx = result
        .events
        .iter()
        .position(|e| matches!(e, Event::CardsDrawn { .. }))
        .expect("CardsDrawn missing");
    let horror_idx = result
        .events
        .iter()
        .position(|e| matches!(e, Event::HorrorTaken { .. }))
        .expect("HorrorTaken missing");
    assert!(
        shuffle_idx < draw_idx && draw_idx < horror_idx,
        "Expected order DeckShuffled ({shuffle_idx}) < CardsDrawn ({draw_idx}) < \
         HorrorTaken ({horror_idx})"
    );
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.hand.len(), 1);
    assert_eq!(inv.deck.len(), 2);
    assert!(inv.discard.is_empty());
    assert_eq!(inv.horror(), 1);
}

#[test]
fn draw_with_both_empty_deals_one_horror_no_shuffle_no_card() {
    // Per the Rules Reference: empty discard means no shuffle
    // happens. We still apply the 1-horror penalty as the safer
    // reading of "would-draw-from-empty-deck" (see handler doc).
    let (id, state) = draw_scenario();
    let result = test_support::take_turn_action(state, &TurnAction::Draw { investigator: id });
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == id
    );
    assert_event!(
        result.events,
        Event::CardsDrawn { investigator, count: 0 } if *investigator == id
    );
    assert_no_event!(result.events, Event::DeckShuffled { .. });
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.horror(), 1);
    assert_eq!(inv.actions_remaining, 2);
    assert!(inv.hand.is_empty());
    assert!(inv.deck.is_empty());
    assert!(inv.discard.is_empty());
}

#[test]
fn draw_outside_investigation_phase_is_rejected() {
    let (id, mut state) = draw_scenario();
    state.phase = Phase::Mythos;
    // Mythos phase → Draw is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Draw { investigator } if *investigator == id)));
}

#[test]
fn draw_by_non_active_investigator_is_rejected() {
    let (_, mut state) = draw_scenario();
    let other = InvestigatorId(2);
    state
        .investigators
        .insert(other, test_support::test_investigator(2));
    // Non-active investigator → their Draw is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Draw { investigator } if *investigator == other)));
}

#[test]
fn draw_with_zero_actions_is_rejected() {
    let (id, mut state) = draw_scenario();
    state.investigators.get_mut(&id).unwrap().actions_remaining = 0;
    // No actions remaining → Draw is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Draw { investigator } if *investigator == id)));
}

#[test]
fn draw_by_defeated_investigator_is_rejected() {
    let (id, mut state) = draw_scenario();
    state.investigators.get_mut(&id).unwrap().status = Status::Defeated;
    // Defeated status → Draw is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Draw { investigator } if *investigator == id)));
}

#[test]
fn draw_with_low_sanity_investigator_defeated_by_reshuffle_horror() {
    // Interaction: 1-sanity investigator + empty deck + empty
    // discard → the 1-horror penalty defeats the investigator
    // via take_horror's defeat path (#80). Verifies the Draw
    // flow correctly composes with the defeat helpers.
    let (id, mut state) = draw_scenario();
    let inv = state.investigators.get_mut(&id).unwrap();
    // Pre-load accumulated_horror so remaining sanity = 1 (lethal with the 1-horror penalty).
    // max_sanity()=8 from TEST_INV; 7+1=8=defeated.
    inv.investigator_card.accumulated_horror = 7;
    let result = test_support::take_turn_action(state, &TurnAction::Draw { investigator: id });
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == id
    );
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Horror,
        } if *investigator == id
    );
    assert_eq!(result.state.investigators[&id].status, Status::Defeated);
}
