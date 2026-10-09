use super::*;

/// Submit a setup mulligan via `ResolveInput`: `indices` are the hand
/// indices to redraw (empty = keep the hand). The acting investigator is
/// the top `Mulligan` frame's `remaining[0]` — the response carries no
/// investigator id.
fn mulligan_resolve(indices: &[u32]) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: indices.iter().copied().map(OptionId).collect(),
        },
    })
}

/// Build a Mulligan scenario: one investigator with a known hand
/// of 5 cards + a remaining deck of 5, the `Mulligan` frame staged.
/// Bypasses scenario setup (via `seat_and_open`) so tests can control
/// the exact hand composition.
fn mulligan_scenario() -> (InvestigatorId, GameState) {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![
        CardCode::new("h-0"),
        CardCode::new("h-1"),
        CardCode::new("h-2"),
        CardCode::new("h-3"),
        CardCode::new("h-4"),
    ];
    inv.deck = vec![
        CardCode::new("d-0"),
        CardCode::new("d-1"),
        CardCode::new("d-2"),
        CardCode::new("d-3"),
        CardCode::new("d-4"),
    ];
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_rng_seed(2026)
        .with_turn_order([id])
        .with_mulligan_remaining([id])
        .build();
    (id, state)
}

#[test]
fn mulligan_redraw_subset_swaps_named_cards() {
    // Redraw indices [1, 3] → those two are set aside, two new cards
    // come off the deck, and the set-aside pair shuffles back in.
    let (id, state) = mulligan_scenario();
    let result = apply(state, mulligan_resolve(&[1, 3]));
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::MulliganPerformed { investigator, redrawn_count: 2 }
            if *investigator == id
    );
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.hand.len(), 5);
    assert_eq!(inv.deck.len(), 5);
    // h-0, h-2, h-4 stay at relative positions 0/1/2 of the hand
    // (since 1 and 3 got removed, the survivors are in original
    // order at positions 0/1/2). The last 2 hand slots are new
    // draws.
    assert_eq!(inv.hand[0], CardCode::new("h-0"));
    assert_eq!(inv.hand[1], CardCode::new("h-2"));
    assert_eq!(inv.hand[2], CardCode::new("h-4"));
}

#[test]
fn mulligan_redraw_none_keeps_hand_and_consumes_one_shot() {
    let (id, state) = mulligan_scenario();
    let original_hand = state.investigators[&id].hand.clone();
    let original_deck = state.investigators[&id].deck.clone();
    let result = apply(state, mulligan_resolve(&[]));
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::MulliganPerformed { investigator, redrawn_count: 0 }
            if *investigator == id
    );
    let inv = &result.state.investigators[&id];
    // Hand unchanged.
    assert_eq!(inv.hand, original_hand);
    // Deck unchanged (no shuffle happens when nothing moves into it).
    assert_eq!(inv.deck, original_deck);
    // No DeckShuffled (deck wasn't touched).
    assert_no_event!(result.events, Event::DeckShuffled { .. });
}

#[test]
fn mulligan_redraw_all_replaces_entire_hand() {
    let (id, state) = mulligan_scenario();
    let result = apply(state, mulligan_resolve(&[0, 1, 2, 3, 4]));
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::MulliganPerformed { investigator, redrawn_count: 5 }
            if *investigator == id
    );
    let inv = &result.state.investigators[&id];
    assert_eq!(inv.hand.len(), 5);
    assert_eq!(inv.deck.len(), 5);
    // Conservation check: hand + deck (multiset) equals the original
    // hand + deck (multiset) — nothing is lost or duplicated by the
    // set-aside/draw/shuffle-back round trip. That none of the original
    // hand survives into the new hand is asserted separately, by
    // `mulligan_cards_cannot_be_redrawn_as_their_own_replacements`.
    let mut all: Vec<_> = inv.hand.iter().chain(inv.deck.iter()).cloned().collect();
    all.sort();
    let mut expected: Vec<CardCode> = [
        "h-0", "h-1", "h-2", "h-3", "h-4", "d-0", "d-1", "d-2", "d-3", "d-4",
    ]
    .iter()
    .map(|s| CardCode::new(*s))
    .collect();
    expected.sort();
    assert_eq!(all, expected);
}

#[test]
fn mulligan_cards_cannot_be_redrawn_as_their_own_replacements() {
    // Rules Reference, glossary `Mulligan`: "These cards are set aside, and
    // an equivalent number of cards are drawn and added to the player's
    // starting hand. The set-aside cards are then shuffled back into the
    // player's deck." Set-aside precedes the draw, so a mulliganed card is
    // out of the deck while its replacement is drawn and cannot come back
    // as that replacement (#637).
    //
    // `mulligan_scenario` makes this decisive: the hand is exactly the five
    // `h-*` cards and the deck exactly the five `d-*` ones, so redrawing the
    // whole hand must yield a hand of only `d-*` cards no matter how the
    // shuffle falls.
    let (id, state) = mulligan_scenario();
    let mulliganed = state.investigators[&id].hand.clone();
    let result = apply(state, mulligan_resolve(&[0, 1, 2, 3, 4]));
    let inv = &result.state.investigators[&id];

    // Hand size is unchanged by the mulligan.
    assert_eq!(inv.hand.len(), mulliganed.len());
    // None of the mulliganed cards appears among the replacements.
    for code in &mulliganed {
        assert!(
            !inv.hand.contains(code),
            "mulliganed card {code:?} was redrawn as its own replacement; hand: {:?}",
            inv.hand
        );
    }
    // Every mulliganed card is back in the deck once the mulligan completes.
    for code in &mulliganed {
        assert!(
            inv.deck.contains(code),
            "mulliganed card {code:?} is not back in the deck; deck: {:?}",
            inv.deck
        );
    }
    // Nothing is left stranded outside a zone.
    assert_eq!(inv.deck.len(), 5);
    assert!(inv.setaside.is_empty());
    assert!(inv.discard.is_empty());
}

#[test]
fn mulligan_partial_redraw_keeps_the_rejected_cards_out_of_the_replacements() {
    // The same guarantee for a partial mulligan: only the named cards are
    // set aside, and only they are barred from the replacement draw.
    let (id, state) = mulligan_scenario();
    let rejected = [CardCode::new("h-1"), CardCode::new("h-3")];
    let result = apply(state, mulligan_resolve(&[1, 3]));
    let inv = &result.state.investigators[&id];

    assert_eq!(inv.hand.len(), 5);
    for code in &rejected {
        assert!(
            !inv.hand.contains(code),
            "mulliganed card {code:?} was redrawn as its own replacement"
        );
        assert!(
            inv.deck.contains(code),
            "mulliganed card {code:?} is not back in the deck"
        );
    }
    assert_eq!(inv.deck.len(), 5);
    assert!(inv.setaside.is_empty());
}

#[test]
fn mulligan_draws_replacements_before_shuffling_the_set_aside_cards_back() {
    // The bug this fixes was purely one of order, so pin the order itself
    // rather than only its consequence: the replacement draw must land
    // before the shuffle that returns the set-aside cards. Inverting these
    // two is exactly what let a rejected card be redrawn (#637).
    let (_id, state) = mulligan_scenario();
    let result = apply(state, mulligan_resolve(&[0, 1, 2, 3, 4]));

    let draw_idx = result
        .events
        .iter()
        .position(|e| matches!(e, Event::CardsDrawn { .. }))
        .expect("CardsDrawn missing");
    let shuffle_idx = result
        .events
        .iter()
        .position(|e| matches!(e, Event::DeckShuffled { .. }))
        .expect("DeckShuffled missing");
    assert!(
        draw_idx < shuffle_idx,
        "expected the replacement draw ({draw_idx}) before the shuffle-back \
         ({shuffle_idx}); events: {:?}",
        result.events
    );
}

#[test]
fn mulligan_replays_from_the_action_log_bit_for_bit() {
    // The shuffle that returns the set-aside cards draws from the engine
    // RNG, so it has to stay on the replay contract: re-driving the action
    // log from a fresh copy of the same initial state must reproduce the
    // post-mulligan state exactly — including the RNG cursor, so every
    // subsequent draw agrees too. Same shape as the real-Gathering replay
    // check in `crates/scenarios/tests/the_gathering_resolutions.rs`: the
    // caller holds the log and replays it against a fresh initial state.
    let id = InvestigatorId(1);
    let make_initial = || mulligan_scenario().1;
    // The RNG cursor is compared directly below, so a shuffle that consumed a
    // different number of RNG values diverges even when the post-mulligan hand
    // happens to agree. (No engine record follows the mulligan: the turn menu
    // it opens is an outstanding prompt, and engine records reject there.)
    let log = vec![mulligan_resolve(&[0, 2, 4])];
    let drive = |log: &[Action]| {
        let mut state = make_initial();
        let mut events = Vec::new();
        for a in log {
            let result = apply(state, a.clone());
            assert!(
                !matches!(result.outcome, EngineOutcome::Rejected { .. }),
                "replay log action was rejected: {:?}",
                result.outcome
            );
            state = result.state;
            events.extend(result.events);
        }
        (state, events)
    };

    let (state_a, events_a) = drive(&log);
    let (state_b, events_b) = drive(&log);

    assert_eq!(state_a, state_b, "replay must reproduce state bit-for-bit");
    assert_eq!(events_a, events_b, "replay must reproduce the same events");
    // Spelled out separately: `GameState` equality already covers these,
    // but a future field-level `PartialEq` change shouldn't silently drop
    // the two that carry the mulligan's randomness.
    assert_eq!(
        state_a.investigators[&id].hand,
        state_b.investigators[&id].hand
    );
    assert_eq!(state_a.rng, state_b.rng);
}

#[test]
fn mulligan_second_attempt_is_rejected() {
    // After the sole investigator mulligans, the frame drains and the
    // Investigation phase begins (no further mulligan prompt outstanding).
    // A second `ResolveInput` has no `Mulligan` frame to resume and is
    // rejected.
    let (_id, state) = mulligan_scenario();
    let result = apply(state, mulligan_resolve(&[0]));
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result.state.current_mulligan(), None);
    // Try again on the post-mulligan state.
    let result2 = apply(result.state, mulligan_resolve(&[0]));
    assert!(matches!(result2.outcome, EngineOutcome::Rejected { .. }));
    assert!(result2.events.is_empty());
}

#[test]
fn mulligan_resolve_with_no_frame_outstanding_is_rejected() {
    // With no `Mulligan` frame on the stack (here: an empty continuation
    // stack), a `ResolveInput(PickMultiple)` has nothing to resume and is
    // rejected with state + events untouched. Replaces the former
    // "mulligan after cursor cleared" test (the cursor is gone).
    let (_id, mut state) = mulligan_scenario();
    state.continuations = crate::state::ContinuationStack::new();
    let result = apply(state, mulligan_resolve(&[0]));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn mulligan_with_out_of_bounds_index_is_rejected() {
    let (_id, state) = mulligan_scenario();
    let result = apply(state, mulligan_resolve(&[10]));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn mulligan_with_duplicate_indices_is_rejected() {
    let (_id, state) = mulligan_scenario();
    let result = apply(state, mulligan_resolve(&[1, 1]));
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn start_scenario_seeds_mulligan_loop() {
    let state = GameStateBuilder::new().build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: make_test_deck(10),
    }];
    let result = seat_and_open(state, &roster);
    let id = InvestigatorId(1);
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        result.outcome
    );
    assert_eq!(result.state.current_mulligan(), Some(id));
}

#[test]
fn non_resolve_input_action_while_mulligan_pending_is_rejected() {
    // Non-`ResolveInput` player actions are gated by the `Mulligan` frame:
    // the engine refuses Move/Investigate/etc. until every investigator
    // has submitted their mulligan choice. Without an InvestigatorTurn
    // frame, no open-turn actions are legal.
    let id = InvestigatorId(1);
    let a = LocationId(10);
    let b = LocationId(11);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(a);
    inv.actions_remaining = 3;
    let mut loc_a = test_support::test_location(10, "A");
    loc_a.connections = vec![b];
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc_a)
        .with_location(test_support::test_location(11, "B"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_turn_order([id])
        .with_mulligan_remaining([id])
        .build();
    // Mulligan frame gates all open-turn actions → Move is not legal.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == id && *destination == b)));
}

#[test]
fn solo_mulligan_drains_the_loop() {
    // Single-investigator scenario: as soon as that one
    // investigator mulligans (empty redraw counts), all
    // investigators have mulliganed and the loop drains.
    let (_id, state) = mulligan_scenario();
    let result = apply(state, mulligan_resolve(&[]));
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(result.state.current_mulligan(), None);
}

#[test]
fn multi_investigator_mulligan_advances_in_player_order() {
    // Two investigators; the loop advances inv1 → inv2 → drained as
    // each mulligans in player order.
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    let mut a = test_support::test_investigator(1);
    let mut b = test_support::test_investigator(2);
    a.hand = vec![CardCode::new("a-0")];
    b.hand = vec![CardCode::new("b-0")];
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_turn_order([inv1, inv2])
        .with_mulligan_remaining([inv1, inv2])
        .build();

    let after_first = apply(state, mulligan_resolve(&[]));
    assert!(
        matches!(after_first.outcome, EngineOutcome::AwaitingInput { .. }),
        "after inv1 mulligans, the loop re-prompts inv2 (AwaitingInput), got {:?}",
        after_first.outcome
    );
    assert_eq!(after_first.state.current_mulligan(), Some(inv2));

    let after_second = apply(after_first.state, mulligan_resolve(&[]));
    assert!(matches!(
        after_second.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(after_second.state.current_mulligan(), None);
}

#[test]
fn multi_investigator_real_redraw_plus_empty_mulligan_combo() {
    // One investigator does a real redraw, the other keeps their
    // hand. Both mulligan; the loop drains after the second.
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    let mut a = test_support::test_investigator(1);
    let mut b = test_support::test_investigator(2);
    a.hand = vec![
        CardCode::new("a-h-0"),
        CardCode::new("a-h-1"),
        CardCode::new("a-h-2"),
    ];
    a.deck = vec![
        CardCode::new("a-d-0"),
        CardCode::new("a-d-1"),
        CardCode::new("a-d-2"),
    ];
    b.hand = vec![CardCode::new("b-h-0"), CardCode::new("b-h-1")];
    b.deck = vec![CardCode::new("b-d-0")];
    let state = GameStateBuilder::new()
        .with_investigator(a)
        .with_investigator(b)
        .with_turn_order([inv1, inv2])
        .with_mulligan_remaining([inv1, inv2])
        .with_rng_seed(99)
        .build();

    // inv1 redraws indices [0, 2] → those two go to deck, deck
    // shuffles, two new cards come back.
    let after_inv1 = apply(state, mulligan_resolve(&[0, 2]));
    assert!(
        matches!(after_inv1.outcome, EngineOutcome::AwaitingInput { .. }),
        "after inv1 mulligans, the loop re-prompts inv2, got {:?}",
        after_inv1.outcome
    );
    assert_eq!(after_inv1.state.current_mulligan(), Some(inv2)); // inv2 hasn't yet
    let inv1_after = &after_inv1.state.investigators[&inv1];
    assert_eq!(inv1_after.hand.len(), 3);
    assert_eq!(inv1_after.deck.len(), 3);
    assert_event!(
        after_inv1.events,
        Event::MulliganPerformed { investigator, redrawn_count: 2 }
            if *investigator == inv1
    );

    // inv2 keeps hand (empty redraw). The loop drains.
    let original_inv2_hand = after_inv1.state.investigators[&inv2].hand.clone();
    let original_inv2_deck = after_inv1.state.investigators[&inv2].deck.clone();
    let after_inv2 = apply(after_inv1.state, mulligan_resolve(&[]));
    assert!(matches!(
        after_inv2.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(after_inv2.state.current_mulligan(), None);
    assert_event!(
        after_inv2.events,
        Event::MulliganPerformed { investigator, redrawn_count: 0 }
            if *investigator == inv2
    );
    // inv2's zones untouched by their no-op mulligan.
    let inv2_after = &after_inv2.state.investigators[&inv2];
    assert_eq!(inv2_after.hand, original_inv2_hand);
    assert_eq!(inv2_after.deck, original_inv2_deck);
}
