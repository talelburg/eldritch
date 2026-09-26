use super::*;
use crate::action::{Action, EngineRecord};
use crate::engine;
use crate::event::Event;
use crate::rng::RngState;
use crate::state::{CardCode, GameStateBuilder};

#[test]
fn shuffle_encounter_deck_emits_event_when_two_or_more_cards() {
    let mut state = GameStateBuilder::new().build();
    state.rng = RngState::new(42);
    state.encounter_deck.push_back(CardCode("a".into()));
    state.encounter_deck.push_back(CardCode("b".into()));
    state.encounter_deck.push_back(CardCode("c".into()));

    let mut events = Vec::new();
    shuffle_encounter_deck(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(matches!(events.as_slice(), [Event::EncounterDeckShuffled]));
    assert_eq!(state.encounter_deck.len(), 3);
    let mut codes: Vec<_> = state.encounter_deck.iter().cloned().collect();
    codes.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        codes,
        vec![
            CardCode("a".into()),
            CardCode("b".into()),
            CardCode("c".into())
        ]
    );
}

#[test]
fn shuffle_encounter_deck_is_silent_on_zero_or_one_card() {
    for n in 0..=1 {
        let mut state = GameStateBuilder::new().build();
        for i in 0..n {
            state.encounter_deck.push_back(CardCode(format!("c{i}")));
        }
        let mut events = Vec::new();
        shuffle_encounter_deck(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert!(events.is_empty(), "expected no event for n={n} deck");
    }
}

#[test]
fn reshuffle_encounter_discard_moves_discard_into_deck_and_shuffles() {
    let mut state = GameStateBuilder::new().build();
    state.rng = RngState::new(7);
    for i in 0..5 {
        state.encounter_discard.push(CardCode(format!("d{i}")));
    }

    let mut events = Vec::new();
    reshuffle_encounter_discard(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(
        state.encounter_discard.is_empty(),
        "discard should be drained"
    );
    assert_eq!(state.encounter_deck.len(), 5, "all 5 cards moved into deck");
    assert!(
        matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
        "expected EncounterDeckShuffled (≥ 2 cards moved)"
    );
}

#[test]
fn reshuffle_encounter_discard_is_silent_when_discard_has_one_card() {
    let mut state = GameStateBuilder::new().build();
    state.encounter_discard.push(CardCode("solo".into()));

    let mut events = Vec::new();
    reshuffle_encounter_discard(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(state.encounter_discard.is_empty());
    assert_eq!(state.encounter_deck.len(), 1);
    assert!(events.is_empty(), "1-card shuffle emits no event");
}

#[test]
fn draw_encounter_top_drains_deck_then_returns_none() {
    let mut state = GameStateBuilder::new().build();
    state.encounter_deck.push_back(CardCode("a".into()));
    state.encounter_deck.push_back(CardCode("b".into()));
    state.encounter_deck.push_back(CardCode("c".into()));

    let mut events = Vec::new();

    assert_eq!(
        draw_encounter_top(&mut Cx {
            state: &mut state,
            events: &mut events,
        }),
        Some(CardCode("a".into()))
    );
    assert_eq!(
        draw_encounter_top(&mut Cx {
            state: &mut state,
            events: &mut events,
        }),
        Some(CardCode("b".into()))
    );
    assert_eq!(
        draw_encounter_top(&mut Cx {
            state: &mut state,
            events: &mut events,
        }),
        Some(CardCode("c".into()))
    );
    assert_eq!(
        draw_encounter_top(&mut Cx {
            state: &mut state,
            events: &mut events,
        }),
        None
    );
    assert!(
        events.is_empty(),
        "no events for any draw — discard is always empty, no reshuffle is triggered"
    );
}

#[test]
fn draw_encounter_top_reshuffles_discard_on_empty_deck() {
    let mut state = GameStateBuilder::new().build();
    state.rng = RngState::new(13);
    state.encounter_discard.push(CardCode("x".into()));
    state.encounter_discard.push(CardCode("y".into()));
    state.encounter_discard.push(CardCode("z".into()));

    let mut events = Vec::new();
    let drawn = draw_encounter_top(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    let drawn_code = drawn.expect("should reshuffle and draw");
    assert!(
        [
            CardCode("x".into()),
            CardCode("y".into()),
            CardCode("z".into())
        ]
        .contains(&drawn_code),
        "drawn card must be one of the three discard cards, got {drawn_code:?}"
    );
    assert_eq!(
        state.encounter_deck.len(),
        2,
        "2 cards remain in deck post-draw"
    );
    assert!(state.encounter_discard.is_empty(), "discard drained");
    assert!(
        matches!(events.as_slice(), [Event::EncounterDeckShuffled]),
        "reshuffle emits one event"
    );
}

#[test]
fn draw_encounter_top_returns_none_when_deck_and_discard_both_empty() {
    let mut state = GameStateBuilder::new().build();
    let mut events = Vec::new();
    assert_eq!(
        draw_encounter_top(&mut Cx {
            state: &mut state,
            events: &mut events,
        }),
        None
    );
    assert!(events.is_empty(), "no events on empty-on-both");
}

#[test]
fn engine_record_encounter_deck_shuffled_drives_shuffle() {
    let mut state = GameStateBuilder::new().build();
    state.rng = RngState::new(99);
    for i in 0..4 {
        state.encounter_deck.push_back(CardCode(format!("c{i}")));
    }
    let original: Vec<_> = state.encounter_deck.iter().cloned().collect();

    let result = engine::apply(state, Action::Engine(EngineRecord::EncounterDeckShuffled));

    assert!(
        matches!(result.outcome, EngineOutcome::Done),
        "expected Done, got {:?}",
        result.outcome
    );
    let mut shuffled: Vec<_> = result.state.encounter_deck.iter().cloned().collect();
    let mut orig_sorted = original.clone();
    shuffled.sort_by(|a, b| a.0.cmp(&b.0));
    orig_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(shuffled, orig_sorted);
    assert!(result
        .events
        .iter()
        .any(|e| matches!(e, Event::EncounterDeckShuffled)));
}

#[test]
fn encounter_deck_shuffle_is_deterministic_from_seed() {
    fn shuffle_with_seed(seed: u64) -> Vec<CardCode> {
        let mut state = GameStateBuilder::new().build();
        state.rng = RngState::new(seed);
        for i in 0..10 {
            state.encounter_deck.push_back(CardCode(format!("c{i:02}")));
        }
        let mut events = Vec::new();
        shuffle_encounter_deck(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        state.encounter_deck.iter().cloned().collect()
    }

    let a = shuffle_with_seed(2026);
    let b = shuffle_with_seed(2026);
    assert_eq!(a, b, "same seed must produce same shuffle order");

    let c = shuffle_with_seed(42);
    assert_ne!(
        a, c,
        "different seeds should produce different orders (smoke test)"
    );
}
