//! #111 acceptance: upkeep step 4.5 discards down to the hand-size cap.
//!
//! Drives a full apply cycle through a locally-built scenario shell (via
//! `seat_and_open`) → `Mulligan` → `EndTurn`, padding the sole investigator's
//! hand so that — after the step-4.4 draw — they hold more than the cap at step
//! 4.5. The round-ending `EndTurn` must cascade into upkeep and pause with
//! `AwaitingInput`; resolving the prompt with `PickMultiple` must land the hand
//! at exactly the cap and let the round proceed.
//!
//! Moved down from `crates/scenarios/tests/upkeep_hand_size.rs` (#873): no card
//! is involved, only a scenario module, which under ADR 0016 is a locally-built
//! shell. No scenario registry is installed, so the resolution hook is skipped
//! and the round cannot end mid-cascade.

use game_core::action::{Action, InputResponse, PlayerAction, RosterEntry};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, EngineOutcome, OptionId};
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, Continuation, GameState, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{self, GameStateBuilder, MockRegistry, TEST_INV};

/// Per-binary code prefix. Nothing here is looked up in the registry — the
/// hand-size discard path only moves cards between hand and discard — so the
/// codes are deliberately not ArkhamDB-shaped (ADR 0016). This replaces the
/// former `01999` filler, a five-digit code that looked like a real one and is
/// in no pack.
const PREFIX: &str = "_uhs_";

/// The hand-size cap enforced at upkeep step 4.5.
// mirrors the engine-private phases::HAND_SIZE_LIMIT; keep in sync if the cap changes.
const HAND_SIZE_LIMIT: usize = 8;

/// Hand size to pad to before the round-ending `EndTurn`. The step-4.4 draw adds
/// one, landing on 12 at the 4.5 check — a discard of 12 - cap = 4.
const PADDED_HAND: usize = 11;

#[ctor::ctor(unsafe)]
fn install() {
    // No probe cards: `install` composes `metadata_for_test_inv` (capacity reads
    // for `TEST_INV`) and `abilities_for_terminal` (the terminal act/agenda
    // reverses `setup` seeds), which is all the cascade touches.
    MockRegistry::new().install();
}

/// The locally-built scenario shell's state: one revealed location to seat onto,
/// a chaos bag, and two-card act/agenda decks ending in a terminal card. Phase =
/// Mythos, round = 0 — ready for [`engine::seat_and_open`].
fn setup() -> GameState {
    let mut location = test_support::test_location(10, "Upkeep Location");
    location.code = CardCode::new(format!("{PREFIX}loc"));

    let mut state = GameStateBuilder::new()
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    state.starting_location = Some(LocationId(10));
    // Something for the Mythos step-1.4 prompt to be pending over; these tests
    // stop at that prompt and never resolve it, so it is never drawn.
    state
        .encounter_deck
        .push_back(CardCode::new(format!("{PREFIX}encounter")));
    state.agenda_deck = vec![
        Agenda {
            code: CardCode::new(format!("{PREFIX}agenda_1")),
            doom_threshold: 2,
        },
        Agenda {
            // Terminal: last in the deck (ADR 0013). Its reverse reaches R2.
            code: test_support::terminal_code(2),
            doom_threshold: 2,
        },
    ];
    state.act_deck = vec![
        Act {
            code: CardCode::new(format!("{PREFIX}act_1")),
            clue_threshold: 2,
        },
        Act {
            // Terminal: last in the deck. Its reverse reaches R1.
            code: test_support::terminal_code(1),
            clue_threshold: 2,
        },
    ];
    state
}

/// A one-investigator roster holding 6 cards: `seat_and_open` draws 5 for the
/// opening hand, leaving 1 for the step-4.4 upkeep draw.
fn roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: (0..6u32)
            .map(|i| CardCode::new(format!("{PREFIX}deck_{i}")))
            .collect(),
    }]
}

/// Pad the sole investigator's hand to [`PADDED_HAND`] with opaque filler.
fn pad_hand(state: &mut GameState) {
    let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
    while inv.hand.len() < PADDED_HAND {
        inv.hand.push(CardCode::new(format!("{PREFIX}filler")));
    }
}

#[test]
#[allow(clippy::too_many_lines)] // end-to-end upkeep walkthrough; length is inherent
fn upkeep_prompts_and_discards_down_to_eight() {
    let inv1 = InvestigatorId(1);

    // seat_and_open → mulligan (keep hand).
    let r1 = engine::seat_and_open(setup(), &roster());
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        r1.outcome
    );

    let r2 = engine::apply(
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
    pad_hand(&mut state);
    let hand_before_end = state.investigators[&inv1].hand.len();
    let discard_pile_before = state.investigators[&inv1].discard.len();
    assert!(
        !matches!(
            state.continuations.last(),
            Some(Continuation::HandSizeDiscard(_))
        ),
        "no discard should be pending before the round-ending EndTurn"
    );

    // Act 1: the round-ending EndTurn cascades Investigation → Enemy →
    // Upkeep (4.2 reset, 4.3 ready, 4.4 draw +1, 4.5 hand-size check).
    // The +1 draw pushes the hand to 12 (> cap), so 4.5 suspends.
    let r3 = test_support::take_turn_action(state, &TurnAction::EndTurn);

    assert!(
        matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }),
        "upkeep 4.5 must prompt for a discard, got {:?}",
        r3.outcome,
    );
    assert!(
        matches!(
            r3.state.continuations.last(),
            Some(Continuation::HandSizeDiscard(_))
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
    let r4 = engine::apply(
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
            Some(Continuation::HandSizeDiscard(_))
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
    // discard_count = 12 - HAND_SIZE_LIMIT = 4; indices 0..4.
    let discard_count = u32::try_from(PADDED_HAND + 1 - HAND_SIZE_LIMIT).unwrap();
    let selected: Vec<OptionId> = (0..discard_count).map(OptionId).collect();

    // Drive the same sequence twice to verify replay determinism.
    let run_sequence = |initial: GameState| -> GameState {
        let mut state = engine::seat_and_open(initial, &roster()).state;
        state = engine::apply(
            state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            }),
        )
        .state;
        // Pad hand to 11 so that the upkeep draw (4.4) pushes it to 12,
        // triggering the hand-size discard prompt at 4.5.
        pad_hand(&mut state);
        state = test_support::take_turn_action(state, &TurnAction::EndTurn).state;
        engine::apply(
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
    let final_state = run_sequence(setup());

    // --- Second pass: replay from the same initial state. ---
    let replayed_state = run_sequence(setup());

    // Replaying the same action sequence from the same initial state must
    // reproduce identical state bit-for-bit — the PickMultiple discard path is
    // deterministic and must not drift between runs.
    assert_eq!(
        final_state, replayed_state,
        "replay must reproduce identical state"
    );
}
