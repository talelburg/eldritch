use super::*;
use crate::test_support::TEST_REGISTRIES;

#[test]
fn search_deck_top_n_auto_takes_single_eligible() {
    // One card in the deck top; no filter ⇒ sole eligible ⇒ auto-take.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).deck(["90001"]))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
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
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .build();
    // A seat's deck is empty unless set.
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
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
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).deck(["90001", "90002", "90003"]))
        .build();
    let mut events = Vec::new();
    let effect = dsl::search_deck(InvestigatorTarget::You, SearchScope::Top(3), None);
    let outcome = run(&mut Cx::for_test(&mut state, &mut events), &effect, ctx(1));
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

/// Each searched card is anchored by its position in the deck and labelled
/// with its name, and the prompt is anchored to the searched area (ADR 0011).
/// The area is the top 3 cut to the 2 cards the deck holds, so a host shows
/// no card that isn't there.
#[test]
fn search_deck_options_are_anchored_to_deck_positions_in_the_searched_area() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).deck([test_support::TEST_ASSET, test_support::TEST_TREACHERY]))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
        &dsl::search_deck(InvestigatorTarget::You, SearchScope::Top(3), None),
        ctx(1),
    );
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("two eligible cards suspend for a pick, got {outcome:?}");
    };
    let deck_card = |index| OptionTarget::DeckCard {
        investigator: InvestigatorId(1),
        index: DeckIndex(index),
    };
    let offered: Vec<_> = request
        .options
        .iter()
        .map(|o| (o.label.as_str(), o.target.clone()))
        .collect();
    assert_eq!(
        offered,
        [
            ("Test Asset", Some(deck_card(0))),
            ("Test Treachery", Some(deck_card(1))),
        ],
    );
    assert_eq!(
        request.target,
        Some(OptionTarget::SearchedTopOfDeck {
            investigator: InvestigatorId(1),
            count: 2,
        }),
    );
}

/// A whole-deck search names the whole deck as its area.
#[test]
fn an_entire_deck_search_is_anchored_to_the_whole_deck() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).deck([test_support::TEST_ASSET, test_support::TEST_TREACHERY]))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
        &dsl::search_deck(InvestigatorTarget::You, SearchScope::EntireDeck, None),
        ctx(1),
    );
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("two eligible cards suspend for a pick, got {outcome:?}");
    };
    assert_eq!(
        request.target,
        Some(OptionTarget::PlayerDeck(InvestigatorId(1)))
    );
}
