use super::*;

#[test]
fn search_deck_top_n_auto_takes_single_eligible() {
    // One card in the deck top; no filter ⇒ sole eligible ⇒ auto-take.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state.investigators.get_mut(&id).unwrap().deck = vec![CardCode::new("90001")];
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::search_deck(InvestigatorTarget::You, SearchScope::Top(3), None),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    let inv = &state.investigators[&id];
    assert!(inv.hand.contains(&CardCode::new("90001")));
    assert!(inv.deck.is_empty());
}

#[test]
fn search_deck_with_no_eligible_cards_is_find_nothing_not_reject() {
    // Empty deck: 0 eligible ⇒ find nothing, still Done (RR p.18 — a search
    // may legally find nothing; it is NOT a rejection).
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state.investigators.get_mut(&id).unwrap().deck.clear();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::search_deck(InvestigatorTarget::You, SearchScope::Top(3), None),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert!(state.investigators[&id].hand.is_empty());
}

#[test]
fn search_deck_top_n_suspends_on_two_eligible_then_takes_pick() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state.investigators.get_mut(&id).unwrap().deck = vec![
        CardCode::new("90001"),
        CardCode::new("90002"),
        CardCode::new("90003"),
    ];
    let mut events = Vec::new();
    let effect = dsl::search_deck(InvestigatorTarget::You, SearchScope::Top(3), None);
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &effect,
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    let _ = &effect;

    // Resume picking option 1 (the second eligible, "90002").
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert_eq!(outcome, EngineOutcome::Done);
    let inv = &state.investigators[&id];
    assert!(inv.hand.contains(&CardCode::new("90002")));
    assert!(!inv.deck.contains(&CardCode::new("90002")));
    assert_eq!(inv.deck.len(), 2);
}
