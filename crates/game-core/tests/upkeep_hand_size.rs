//! #111 acceptance: upkeep step 4.5 discards down to the hand-size cap.
//!
//! Drives a full apply cycle through a locally-built scenario shell (via
//! `seat_and_open`) → `Mulligan` → `EndTurn`, padding the sole investigator's
//! hand so that — after the step-4.4 draw — they hold more than the cap at step
//! 4.5. The round-ending `EndTurn` must cascade into upkeep and pause at the
//! discard prompt; choosing the discards must land the hand
//! at exactly the cap and let the round proceed.
//!
//! Moved down from `crates/scenarios/tests/upkeep_hand_size.rs` (#873): no card
//! is involved, only a scenario module, which under ADR 0016 is a locally-built
//! shell. The registries name no scenario module, so the resolution hook is skipped
//! and the round cannot end mid-cascade.

use std::sync::LazyLock;

use game_core::action::RosterEntry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{InputKind, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    Act, Agenda, CardCode, ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{self, MockRegistry, TestSession, TEST_INV};

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

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // No probe cards: `build` composes `metadata_for_test_inv` (capacity reads
        // for `TEST_INV`) and `abilities_for_terminal` (the terminal act/agenda
        // reverses `setup` seeds), which is all the cascade touches.
        cards: MockRegistry::new().build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
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

const INV: InvestigatorId = InvestigatorId(1);

/// Seat the roster with `seat_and_open`, close the mulligan keeping the hand,
/// and pad the sole investigator's hand to [`PADDED_HAND`] with opaque filler:
/// Investigation, round 1, at the turn menu. The padding has no step that
/// makes it, so the opened state is edited and settled again.
fn opened_with_a_padded_hand() -> TestSession {
    let opened = game_core::engine::seat_and_open(setup(), &roster(), registries());
    let mut state = TestSession::new(opened.state, registries())
        .choose_from_hand(&[])
        .state()
        .clone();
    let inv = state.investigators.get_mut(&INV).unwrap();
    while inv.hand.len() < PADDED_HAND {
        inv.hand.push(CardCode::new(format!("{PREFIX}filler")));
    }
    let session = TestSession::new(state, registries());
    // No discard is pending before the round-ending EndTurn: the session rests
    // at the turn menu.
    session.assert_at_turn_menu(INV);
    session
}

/// The first `n` cards of `INV`'s hand, by code: the hand-size discard's choice.
fn first_cards(session: &TestSession, n: usize) -> Vec<CardCode> {
    session.state().investigators[&INV].hand[..n].to_vec()
}

#[test]
fn upkeep_prompts_and_discards_down_to_eight() {
    // Padding to 11 lands us at 12 cards at the 4.5 check once the step-4.4
    // draw adds one, requiring a discard of (11 + 1 draw) - HAND_SIZE_LIMIT = 4.
    let opened = opened_with_a_padded_hand();
    let hand_before_end = opened.state().investigators[&INV].hand.len();
    let discard_pile_before = opened.state().investigators[&INV].discard.len();

    // Act 1: the round-ending EndTurn cascades Investigation → Enemy →
    // Upkeep (4.2 reset, 4.3 ready, 4.4 draw +1, 4.5 hand-size check).
    // The +1 draw pushes the hand to 12 (> cap), so 4.5 suspends.
    let paused = opened.take(&TurnAction::EndTurn);

    assert_eq!(
        (paused.state().phase, paused.prompt().kind),
        (Phase::Upkeep, InputKind::PickMultiple),
        "upkeep 4.5 must prompt for a discard, got {:?}",
        paused.prompt(),
    );
    let hand_at_check = paused.state().investigators[&INV].hand.len();
    assert_eq!(
        hand_at_check,
        hand_before_end + 1,
        "step-4.4 draw added exactly one card before the 4.5 check"
    );
    assert!(
        hand_at_check > HAND_SIZE_LIMIT,
        "hand ({hand_at_check}) must be over the cap at 4.5"
    );

    // Act 2: discard exactly (hand_len - cap) cards. `choose_from_hand`
    // panics unless the session rests at the hand-size discard.
    let discard_count = hand_at_check - HAND_SIZE_LIMIT;
    let chosen = first_cards(&paused, discard_count);
    let session = paused.choose_from_hand(&chosen.iter().map(CardCode::as_str).collect::<Vec<_>>());

    // Resolving the discard continues the upkeep cascade into Mythos, which
    // pauses at the encounter-draw prompt: the discard is no longer pending.
    session.assert_at_encounter_draw();
    assert_eq!(
        session.state().investigators[&INV].hand.len(),
        HAND_SIZE_LIMIT,
        "investigator must land at exactly the hand-size cap"
    );
    assert_eq!(
        session.state().investigators[&INV].discard.len(),
        discard_pile_before + discard_count,
        "discarded cards must move to the investigator's discard pile"
    );

    // The round proceeded: the upkeep cascade completed and advanced to
    // the Mythos phase of the next round.
    assert_eq!(
        session.state().phase,
        Phase::Mythos,
        "round must proceed into Mythos after the discard resolves"
    );
    assert!(
        session.state().current_encounter_drawer().is_some(),
        "Mythos draw cursor must be seeded once the round proceeds"
    );
}

// ------------------------------------------------------------------
// Replay determinism
// ------------------------------------------------------------------

#[test]
fn upkeep_hand_size_discard_replay_is_deterministic() {
    // discard_count = 12 - HAND_SIZE_LIMIT = 4: the first four cards.
    let discard_count = PADDED_HAND + 1 - HAND_SIZE_LIMIT;

    // Drive the same sequence twice to verify replay determinism.
    let run_sequence = || -> GameState {
        // Padded to 11 so that the upkeep draw (4.4) pushes the hand to 12,
        // triggering the hand-size discard prompt at 4.5.
        let paused = opened_with_a_padded_hand().take(&TurnAction::EndTurn);
        let chosen = first_cards(&paused, discard_count);
        paused
            .choose_from_hand(&chosen.iter().map(CardCode::as_str).collect::<Vec<_>>())
            .state()
            .clone()
    };

    // --- First pass: drive and collect final state. ---
    let final_state = run_sequence();

    // --- Second pass: replay from the same initial state. ---
    let replayed_state = run_sequence();

    // Replaying the same action sequence from the same initial state must
    // reproduce identical state bit-for-bit — the PickMultiple discard path is
    // deterministic and must not drift between runs.
    assert_eq!(
        final_state, replayed_state,
        "replay must reproduce identical state"
    );
}
