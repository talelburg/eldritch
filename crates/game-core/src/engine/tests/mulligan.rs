use super::*;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

/// A raw setup-mulligan answer: `indices` are the ids of the options to
/// redraw (empty = keep the hand). The mulligan offers the whole hand in
/// order, so option `i` is the hand card at `i`. Only for the tests whose subject is the wire
/// action itself — a malformed answer, an answer with no mulligan to
/// resume, or a replayed log; every other test answers with
/// `choose_from_hand`.
fn mulligan_resolve(indices: &[u32]) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: indices.iter().copied().map(OptionId).collect(),
        },
    })
}

/// A Mulligan board: one investigator with a known hand of 5 cards + a
/// remaining deck of 5, the `Mulligan` frame staged. Bypasses scenario
/// setup (via `seat_and_open`) so tests can control the exact hand
/// composition.
fn mulligan_board() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .hand(["h-0", "h-1", "h-2", "h-3", "h-4"])
                .deck(["d-0", "d-1", "d-2", "d-3", "d-4"]),
        )
        .with_rng_seed(2026)
        .with_seating([INV])
        .with_mulligan_remaining([INV])
}

/// The Mulligan board settled at `INV`'s mulligan prompt.
fn at_mulligan() -> TestSession {
    mulligan_board().session()
}

/// The sole investigator mulligans `redraw`: the loop drains and play goes
/// on to their turn.
fn mulligan(redraw: &[&str]) -> TestSession {
    let session = at_mulligan().choose_from_hand(redraw);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn mulligan_redraw_subset_swaps_named_cards() {
    // Redraw h-1 and h-3 → those two are set aside, two new cards
    // come off the deck, and the set-aside pair shuffles back in.
    let session = mulligan(&["h-1", "h-3"]);
    assert_event!(
        session.events(),
        Event::MulliganPerformed { investigator, redrawn_count: 2 }
            if *investigator == INV
    );
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.hand.len(), 5);
    assert_eq!(inv.deck.len(), 5);
    // h-0, h-2, h-4 stay at relative positions 0/1/2 of the hand
    // (since h-1 and h-3 got removed, the survivors are in original
    // order at positions 0/1/2). The last 2 hand slots are new
    // draws.
    assert_eq!(inv.hand[0], CardCode::new("h-0"));
    assert_eq!(inv.hand[1], CardCode::new("h-2"));
    assert_eq!(inv.hand[2], CardCode::new("h-4"));
}

#[test]
fn mulligan_redraw_none_keeps_hand_and_consumes_one_shot() {
    let paused = at_mulligan();
    let original_hand = paused.state().investigators[&INV].hand.clone();
    let original_deck = paused.state().investigators[&INV].deck.clone();
    let session = paused.choose_from_hand(&[]);
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::MulliganPerformed { investigator, redrawn_count: 0 }
            if *investigator == INV
    );
    let inv = &session.state().investigators[&INV];
    // Hand unchanged.
    assert_eq!(inv.hand, original_hand);
    // Deck unchanged (no shuffle happens when nothing moves into it).
    assert_eq!(inv.deck, original_deck);
    // No DeckShuffled (deck wasn't touched).
    assert_no_event!(session.events(), Event::DeckShuffled { .. });
}

/// The whole of the Mulligan board's hand.
const WHOLE_HAND: [&str; 5] = ["h-0", "h-1", "h-2", "h-3", "h-4"];

#[test]
fn mulligan_redraw_all_replaces_entire_hand() {
    let session = mulligan(&WHOLE_HAND);
    assert_event!(
        session.events(),
        Event::MulliganPerformed { investigator, redrawn_count: 5 }
            if *investigator == INV
    );
    let inv = &session.state().investigators[&INV];
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

/// Regression for #637: a card set aside by the mulligan cannot be drawn as
/// its own replacement.
#[test]
fn mulligan_cards_cannot_be_redrawn_as_their_own_replacements() {
    // Rules Reference, glossary `Mulligan`: "These cards are set aside, and
    // an equivalent number of cards are drawn and added to the player's
    // starting hand. The set-aside cards are then shuffled back into the
    // player's deck." Set-aside precedes the draw, so a mulliganed card is
    // out of the deck while its replacement is drawn and cannot come back
    // as that replacement.
    //
    // `mulligan_board` makes this decisive: the hand is exactly the five
    // `h-*` cards and the deck exactly the five `d-*` ones, so redrawing the
    // whole hand must yield a hand of only `d-*` cards no matter how the
    // shuffle falls.
    let mulliganed = WHOLE_HAND.map(CardCode::new);
    let session = mulligan(&WHOLE_HAND);
    let inv = &session.state().investigators[&INV];

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
    let redrawn = ["h-1", "h-3"];
    let rejected = redrawn.map(CardCode::new);
    let session = mulligan(&redrawn);
    let inv = &session.state().investigators[&INV];

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
    // Pin the order itself rather than only its consequence: the
    // replacement draw lands before the shuffle that returns the set-aside
    // cards. Inverting these two lets a rejected card be redrawn.
    let session = mulligan(&WHOLE_HAND);
    let events = session.events();

    let draw_idx = events
        .iter()
        .position(|e| matches!(e, Event::CardsDrawn { .. }))
        .expect("CardsDrawn missing");
    let shuffle_idx = events
        .iter()
        .position(|e| matches!(e, Event::DeckShuffled { .. }))
        .expect("DeckShuffled missing");
    assert!(
        draw_idx < shuffle_idx,
        "expected the replacement draw ({draw_idx}) before the shuffle-back \
         ({shuffle_idx}); events: {events:?}",
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
    // The subject is the logged wire action, so the log is replayed raw.
    let make_initial = || mulligan_board().build();
    // The RNG cursor is compared directly below, so a shuffle that consumed a
    // different number of RNG values diverges even when the post-mulligan hand
    // happens to agree. (No engine record follows the mulligan: the turn menu
    // it opens is an outstanding prompt, and engine records reject there.)
    let log = vec![mulligan_resolve(&[0, 2, 4])];
    let replay = |log: &[Action]| {
        let mut state = make_initial();
        let mut events = Vec::new();
        for a in log {
            let result = apply(state, a.clone(), TEST_REGISTRIES);
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

    let (state_a, events_a) = replay(&log);
    let (state_b, events_b) = replay(&log);

    assert_eq!(state_a, state_b, "replay must reproduce state bit-for-bit");
    assert_eq!(events_a, events_b, "replay must reproduce the same events");
    // Spelled out separately: `GameState` equality already covers these,
    // but a future field-level `PartialEq` change shouldn't silently drop
    // the two that carry the mulligan's randomness.
    assert_eq!(
        state_a.investigators[&INV].hand,
        state_b.investigators[&INV].hand
    );
    assert_eq!(state_a.rng, state_b.rng);
}

#[test]
fn mulligan_second_attempt_is_rejected() {
    // After the sole investigator mulligans, the frame drains and the
    // Investigation phase begins (no further mulligan prompt outstanding).
    // A second mulligan answer has no `Mulligan` frame to resume and is
    // rejected. The subject is that stray answer, so it is sent raw.
    let session = mulligan(&["h-0"]);
    assert_eq!(session.state().current_mulligan(), None);
    // Try again on the post-mulligan state.
    let again = session.apply(mulligan_resolve(&[0]));
    again.expect_rejected();
    again.assert_at_turn_menu(INV);
    assert!(again.finish().events.is_empty());
}

#[test]
fn mulligan_resolve_with_no_frame_outstanding_is_rejected() {
    // With no `Mulligan` frame on the stack (here: an empty continuation
    // stack), a `ResolveInput(PickMultiple)` has nothing to resume and is
    // rejected with state + events untouched. Replaces the former
    // "mulligan after cursor cleared" test (the cursor is gone). The empty
    // stack is the moment under test, so the answer is applied raw to the
    // built state rather than through a session.
    let mut state = mulligan_board().build();
    state.continuations = crate::state::ContinuationStack::new();
    let result = apply(state, mulligan_resolve(&[0]), TEST_REGISTRIES);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

/// A malformed mulligan answer — the subject, so sent raw — is rejected
/// with no events, and the mulligan prompt still stands.
fn assert_malformed_mulligan_rejected(indices: &[u32]) {
    let paused = at_mulligan();
    let before = paused.prompt().clone();
    let bad = paused.apply(mulligan_resolve(indices));
    bad.expect_rejected();
    assert_eq!(bad.prompt(), &before);
    assert!(bad.finish().events.is_empty());
}

#[test]
fn mulligan_with_out_of_bounds_index_is_rejected() {
    assert_malformed_mulligan_rejected(&[10]);
}

#[test]
fn mulligan_with_duplicate_indices_is_rejected() {
    assert_malformed_mulligan_rejected(&[1, 1]);
}

#[test]
fn start_scenario_seeds_mulligan_loop() {
    let state = GameStateBuilder::new().build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: make_test_deck(10),
        random_basic_weaknesses: 0,
    }];
    let result = seat_and_open(state, &roster, 0, TEST_REGISTRIES);
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        result.outcome
    );
    assert_eq!(result.state.current_mulligan(), Some(INV));
}

#[test]
fn non_resolve_input_action_while_mulligan_pending_is_rejected() {
    // Non-`ResolveInput` player actions are gated by the `Mulligan` frame:
    // the engine refuses Move/Investigate/etc. until every investigator
    // has submitted their mulligan choice. Without an InvestigatorTurn
    // frame, no open-turn actions are legal.
    let mut here = test_support::test_location(HERE.0, "A");
    here.connections = vec![CONNECTED];
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE))
        .with_location(here)
        .with_location(test_support::test_location(CONNECTED.0, "B"))
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_seating([INV])
        .with_mulligan_remaining([INV])
        .build();
    // Mulligan frame gates all open-turn actions → Move is not legal.
    assert!(!legal_actions(&state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == INV && *destination == CONNECTED)));
}

#[test]
fn solo_mulligan_drains_the_loop() {
    // Single-investigator scenario: as soon as that one
    // investigator mulligans (empty redraw counts), all
    // investigators have mulliganed and the loop drains.
    let session = mulligan(&[]);
    assert_eq!(session.state().current_mulligan(), None);
}

#[test]
fn multi_investigator_mulligan_advances_in_player_order() {
    // Two investigators; the loop advances inv1 → inv2 → drained as
    // each mulligans in player order.
    let inv2 = InvestigatorId(2);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand(["a-0"]))
        .with_seat(seat(2).hand(["b-0"]))
        .with_seating([INV, inv2])
        .with_mulligan_remaining([INV, inv2])
        .session();

    // After inv1 mulligans, the loop re-prompts inv2.
    let after_first = session.choose_from_hand(&[]);
    assert_eq!(after_first.state().current_mulligan(), Some(inv2));

    let after_second = after_first.choose_from_hand(&[]);
    after_second.assert_at_turn_menu(INV);
    assert_eq!(after_second.state().current_mulligan(), None);
}

/// Each mulligan offers its own investigator's whole hand, anchored to their
/// hand cards, and any number of them: seat 2's prompt names seat 2's two
/// cards and none of seat 1's.
#[test]
fn a_mulligan_offers_the_prompted_investigators_hand() {
    let inv2 = InvestigatorId(2);
    let in_hand = |investigator, hand_index| OptionTarget::HandCard {
        investigator,
        hand_index,
    };
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand(["a-0"]))
        .with_seat(seat(2).hand(["b-0", "b-1"]))
        .with_seating([INV, inv2])
        .with_mulligan_remaining([inv2, INV])
        .session();

    let request = session.prompt();
    let anchors: Vec<_> = request.options.iter().map(|o| o.target.clone()).collect();
    assert_eq!(
        anchors,
        vec![Some(in_hand(inv2, 0)), Some(in_hand(inv2, 1))],
        "seat 2's hand, in hand order",
    );
    assert_eq!(
        request.kind,
        InputKind::PickMultiple(PickCount { min: 0, max: 2 })
    );
}

#[test]
fn multi_investigator_real_redraw_plus_empty_mulligan_combo() {
    // One investigator does a real redraw, the other keeps their
    // hand. Both mulligan; the loop drains after the second.
    let inv2 = InvestigatorId(2);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .hand(["a-h-0", "a-h-1", "a-h-2"])
                .deck(["a-d-0", "a-d-1", "a-d-2"]),
        )
        .with_seat(seat(2).hand(["b-h-0", "b-h-1"]).deck(["b-d-0"]))
        .with_seating([INV, inv2])
        .with_mulligan_remaining([INV, inv2])
        .with_rng_seed(99)
        .session();

    // inv1 redraws a-h-0 and a-h-2 → those two go to deck, deck
    // shuffles, two new cards come back.
    let after_inv1 = session.choose_from_hand(&["a-h-0", "a-h-2"]);
    // After inv1 mulligans, the loop re-prompts inv2 (who hasn't yet).
    assert_eq!(after_inv1.state().current_mulligan(), Some(inv2));
    let inv1_after = &after_inv1.state().investigators[&INV];
    assert_eq!(inv1_after.hand.len(), 3);
    assert_eq!(inv1_after.deck.len(), 3);
    assert_event!(
        after_inv1.events(),
        Event::MulliganPerformed { investigator, redrawn_count: 2 }
            if *investigator == INV
    );

    // inv2 keeps hand (empty redraw). The loop drains.
    let original_inv2_hand = after_inv1.state().investigators[&inv2].hand.clone();
    let original_inv2_deck = after_inv1.state().investigators[&inv2].deck.clone();
    let after_inv2 = after_inv1.choose_from_hand(&[]);
    after_inv2.assert_at_turn_menu(INV);
    assert_eq!(after_inv2.state().current_mulligan(), None);
    assert_event!(
        after_inv2.events(),
        Event::MulliganPerformed { investigator, redrawn_count: 0 }
            if *investigator == inv2
    );
    // inv2's zones untouched by their no-op mulligan.
    let inv2_after = &after_inv2.state().investigators[&inv2];
    assert_eq!(inv2_after.hand, original_inv2_hand);
    assert_eq!(inv2_after.deck, original_inv2_deck);
}
