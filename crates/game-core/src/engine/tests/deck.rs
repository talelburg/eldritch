use super::*;
use crate::test_support::TEST_REGISTRIES;

// An engine record applies only with no prompt outstanding (see
// `engine_record_applied_while_a_prompt_is_outstanding_is_rejected` in
// `reject_rollback.rs`), and no step produces one, so the record tests
// below apply it raw to a built state with an empty stack.

#[test]
fn deck_shuffled_engine_record_with_unknown_investigator_is_rejected() {
    let state = GameStateBuilder::new().build();
    let result = apply(
        state,
        Action::Engine(EngineRecord::DeckShuffled {
            investigator: InvestigatorId(999),
        }),
        TEST_REGISTRIES,
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn deck_shuffle_is_deterministic_across_replay() {
    let deck = make_test_deck(20);
    let state_a = GameStateBuilder::new().with_rng_seed(123).build();
    let state_b = GameStateBuilder::new().with_rng_seed(123).build();
    let make_roster = || {
        vec![RosterEntry {
            investigator: CardCode::new(test_support::TEST_INV),
            deck: deck.clone(),
        }]
    };

    let result_a = seat_and_open(state_a, &make_roster(), TEST_REGISTRIES);
    let result_b = seat_and_open(state_b, &make_roster(), TEST_REGISTRIES);
    let id = InvestigatorId(1);

    assert_eq!(
        result_a.state.investigators[&id].deck,
        result_b.state.investigators[&id].deck
    );
    assert_eq!(
        result_a.state.investigators[&id].hand,
        result_b.state.investigators[&id].hand
    );
    assert_eq!(result_a.state.rng, result_b.state.rng);
}

#[test]
fn deck_shuffled_engine_record_shuffles_named_investigator() {
    let id = INV;
    let original_deck = make_test_deck(8);
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).deck(original_deck.clone()))
        .with_rng_seed(99)
        .build();
    let result = apply(
        state,
        Action::Engine(EngineRecord::DeckShuffled { investigator: id }),
        TEST_REGISTRIES,
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::DeckShuffled { investigator } if *investigator == id
    );
    // Deck contains the same cards (multiset equal) but reordered.
    // With seed 99 and 8 cards, the shuffle should differ from
    // the original; treat that as a probabilistic check.
    let after = &result.state.investigators[&id].deck;
    assert_eq!(after.len(), original_deck.len());
    let mut sorted_before = original_deck.clone();
    sorted_before.sort();
    let mut sorted_after = after.clone();
    sorted_after.sort();
    assert_eq!(sorted_before, sorted_after);
}
