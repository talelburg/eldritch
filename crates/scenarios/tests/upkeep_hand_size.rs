//! #111 acceptance: upkeep step 4.5 discards down to the hand-size cap.
//!
//! Drives a full apply cycle through scenario setup (via `seat_and_open`) →
//! `Mulligan` → `EndTurn`, padding the sole investigator's hand so that —
//! after the step-4.4 draw — they hold more than the cap at step 4.5. The
//! round-ending `EndTurn` must cascade into upkeep and pause with
//! `AwaitingInput`; resolving the prompt with `PickMultiple` must land
//! the hand at exactly the cap and let the round proceed.
//!
//! Lives in `crates/scenarios/tests/` for the same process-isolation /
//! crate-direction reasons as `upkeep_phase.rs` and `mythos_phase.rs`:
//! we install [`TEST_REGISTRY`] without colliding with other test
//! binaries, and `game-core` unit tests can't reach a real registry.

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::seat_and_open;
use game_core::state::{CardCode, InvestigatorId, Phase};
use game_core::test_support::{take_turn_action, TEST_INV};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};
use scenarios::test_fixtures::synth_cards::TEST_REGISTRY;
use scenarios::test_fixtures::synthetic;

#[ctor::ctor(unsafe)]
fn install_test_registry() {
    let _ = game_core::card_registry::install(TEST_REGISTRY);
}

/// The hand-size cap enforced at upkeep step 4.5.
// mirrors the engine-private phases::HAND_SIZE_LIMIT; keep in sync if the cap changes.
const HAND_SIZE_LIMIT: usize = 8;

#[test]
#[allow(clippy::too_many_lines)] // end-to-end upkeep walkthrough; length is inherent
fn upkeep_prompts_and_discards_down_to_eight() {
    let inv1 = InvestigatorId(1);
    // Seed 6 cards via the roster: seat_and_open draws 5 for the opening
    // hand, leaving 1 for the step-4.4 upkeep draw.
    let deck = (0..6u32)
        .map(|i| CardCode::new(format!("01{i:03}")))
        .collect();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck,
    }];

    // seat_and_open → mulligan (keep hand).
    let r1 = seat_and_open(synthetic::setup(), &roster);
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        r1.outcome
    );

    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert!(matches!(r2.outcome, EngineOutcome::AwaitingInput { .. }));

    // Pad the hand so we're over cap at 4.5. The step-4.4 draw adds one
    // card; padding to 11 here lands us at 12 cards at the 4.5 check,
    // requiring a discard of (11 + 1 draw) - HAND_SIZE_LIMIT = 4.
    let mut state = r2.state;
    {
        let inv = state.investigators.get_mut(&inv1).unwrap();
        while inv.hand.len() < 11 {
            // Arbitrary code unknown to the test registry — fine because the
            // hand-size discard path only moves cards between hand and discard
            // and never performs a registry lookup.
            inv.hand.push(CardCode::new("01999"));
        }
    }
    let hand_before_end = state.investigators[&inv1].hand.len();
    let discard_pile_before = state.investigators[&inv1].discard.len();
    assert!(
        !matches!(
            state.continuations.last(),
            Some(game_core::state::Continuation::HandSizeDiscard(_))
        ),
        "no discard should be pending before the round-ending EndTurn"
    );

    // Act 1: the round-ending EndTurn cascades Investigation → Enemy →
    // Upkeep (4.2 reset, 4.3 ready, 4.4 draw +1, 4.5 hand-size check).
    // The +1 draw pushes the hand to 12 (> cap), so 4.5 suspends.
    let r3 = take_turn_action(state, &TurnAction::EndTurn);

    assert!(
        matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }),
        "upkeep 4.5 must prompt for a discard, got {:?}",
        r3.outcome,
    );
    assert!(
        matches!(
            r3.state.continuations.last(),
            Some(game_core::state::Continuation::HandSizeDiscard(_))
        ),
        "a HandSizeDiscard frame must be on the stack while awaiting the discard"
    );
    let hand_at_check = r3.state.investigators[&inv1].hand.len();
    assert_eq!(
        hand_at_check,
        hand_before_end + 1,
        "step-4.4 draw added exactly one card before the 4.5 check"
    );
    assert!(
        hand_at_check > HAND_SIZE_LIMIT,
        "hand ({hand_at_check}) must be over the cap at 4.5"
    );

    // Act 2: submit PickMultiple with exactly (hand_len - cap) indices.
    let discard_count = hand_at_check - HAND_SIZE_LIMIT;
    let indices: Vec<u32> = (0..u32::try_from(discard_count).unwrap()).collect();
    let r4 = apply(
        r3.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: indices.into_iter().map(OptionId).collect(),
            },
        }),
    );

    assert!(
        matches!(r4.outcome, EngineOutcome::AwaitingInput { .. }),
        "resolving the discard continues the upkeep cascade into Mythos, which \
         pauses at the encounter-draw prompt"
    );
    assert_eq!(
        r4.state.investigators[&inv1].hand.len(),
        HAND_SIZE_LIMIT,
        "investigator must land at exactly the hand-size cap"
    );
    assert_eq!(
        r4.state.investigators[&inv1].discard.len(),
        discard_pile_before + discard_count,
        "discarded cards must move to the investigator's discard pile"
    );
    assert!(
        !matches!(
            r4.state.continuations.last(),
            Some(game_core::state::Continuation::HandSizeDiscard(_))
        ),
        "discard-pending must be cleared once the queue drains"
    );

    // The round proceeded: the upkeep cascade completed and advanced to
    // the Mythos phase of the next round.
    assert_eq!(
        r4.state.phase,
        Phase::Mythos,
        "round must proceed into Mythos after the discard resolves"
    );
    assert!(
        r4.state.current_encounter_drawer().is_some(),
        "Mythos draw cursor must be seeded once the round proceeds"
    );
}

// ------------------------------------------------------------------
// Replay determinism
// ------------------------------------------------------------------

#[test]
fn upkeep_hand_size_discard_replay_is_deterministic() {
    let inv1 = InvestigatorId(1);

    // 6 deck cards via roster: seat_and_open draws 5 → hand has 5;
    // upkeep step 4.4 draws the last → 12 cards after padding, triggering
    // the hand-size prompt.
    let deck: Vec<CardCode> = (0..6u32)
        .map(|i| CardCode::new(format!("01{i:03}")))
        .collect();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck,
    }];

    // discard_count = 12 - HAND_SIZE_LIMIT = 4; indices 0..4.
    let discard_count = 12u32 - u32::try_from(HAND_SIZE_LIMIT).unwrap();
    let indices: Vec<u32> = (0..discard_count).collect();
    let selected: Vec<OptionId> = indices.iter().copied().map(OptionId).collect();

    // Drive the same sequence twice to verify replay determinism.
    let run_sequence = |initial: game_core::state::GameState| -> game_core::state::GameState {
        let mut state = seat_and_open(initial, &roster).state;
        state = apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            }),
        )
        .state;
        // Pad hand to 11 so that the upkeep draw (4.4) pushes it to 12,
        // triggering the hand-size discard prompt at 4.5.
        {
            let inv = state.investigators.get_mut(&inv1).unwrap();
            while inv.hand.len() < 11 {
                inv.hand.push(CardCode::new("01999"));
            }
        }
        state = take_turn_action(state, &TurnAction::EndTurn).state;
        apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple {
                    selected: selected.clone(),
                },
            }),
        )
        .state
    };

    // --- First pass: drive and collect final state. ---
    let final_state = run_sequence(synthetic::setup());

    // --- Second pass: replay from the same initial state. ---
    let replayed_state = run_sequence(synthetic::setup());

    // Replaying the same action sequence from the same initial state must
    // reproduce identical state bit-for-bit — the PickMultiple discard path is
    // deterministic and must not drift between runs.
    assert_eq!(
        final_state, replayed_state,
        "replay must reproduce identical state"
    );
}
