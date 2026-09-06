//! The Upkeep cascade, driven end to end: ready exhausted cards, draw 1, gain 1
//! resource, bump the round — and replay it identically.
//!
//! Moved down from `crates/scenarios/tests/upkeep_phase.rs` (#873). Nothing here
//! depends on a card: what the tests needed was a *scenario module*, and under
//! ADR 0016 that is a locally-built shell rather than a shared fixture. The
//! `setup()` below is that shell's state, composed with `GameStateBuilder`.
//!
//! No scenario registry is installed — the resolution hook treats `None` as "no
//! scenario behavior wired up; skip", which is what these tests want: the round
//! must not resolve out from under the cascade.

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome};
use game_core::seat_and_open;
use game_core::state::{
    Act, Agenda, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState,
    InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{
    take_turn_action, terminal_code, test_location, GameStateBuilder, MockRegistry, TEST_INV,
};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};

/// Per-binary code prefix. None of these codes is looked up — the deck, the
/// hand and the readied asset are opaque tokens to every step the cascade runs —
/// so they are deliberately not ArkhamDB-shaped (ADR 0016).
const PREFIX: &str = "_uc_";

#[ctor::ctor(unsafe)]
fn install() {
    // No probe cards. `install` composes `metadata_for_test_inv` (capacity reads
    // for `TEST_INV`) and `abilities_for_terminal` (the terminal act/agenda
    // reverses that `setup` seeds), which is everything the cascade touches.
    MockRegistry::new().install();
}

/// One deck card, distinct per index.
fn deck_code(i: u32) -> CardCode {
    CardCode::new(format!("{PREFIX}deck_{i}"))
}

/// The locally-built scenario shell's state: one revealed location to seat onto,
/// a chaos bag so a skill test could be taken, and two-card act/agenda decks
/// ending in a terminal card. Phase = Mythos, round = 0 — ready for
/// [`seat_and_open`], with no investigator pre-seated (callers pass a roster).
///
/// The thresholds are set high enough that a single round's doom placement never
/// advances the terminal agenda: these tests are about Upkeep, and a scenario
/// that ends mid-cascade would test something else.
fn setup() -> GameState {
    let mut location = test_location(10, "Upkeep Location");
    location.code = CardCode::new(format!("{PREFIX}loc"));

    let mut state = GameStateBuilder::new()
        .with_location(location)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    // `seat_and_open` places roster investigators at `starting_location` and
    // reveals it; point it at the already-revealed location above.
    state.starting_location = Some(LocationId(10));
    // Something for the Mythos step-1.4 prompt to be pending over. These tests
    // pause at that prompt and never resolve it, so the card is never drawn.
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
            code: terminal_code(2),
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
            code: terminal_code(1),
            clue_threshold: 2,
        },
    ];
    state
}

/// A one-investigator roster holding 6 cards: `seat_and_open` draws 5 for the
/// opening hand, leaving 1 in the deck for the Upkeep step-4.4 draw.
fn roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: (0..6u32).map(deck_code).collect(),
    }]
}

/// Put one exhausted asset into the seated investigator's play area, so the
/// step-4.3 ready-all has something to ready.
fn seed_exhausted_asset(state: &mut GameState) {
    let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
    let mut card =
        CardInPlay::enter_play(CardCode::new(format!("{PREFIX}asset")), CardInstanceId(1));
    card.exhausted = true;
    inv.cards_in_play.push(card);
}

// ------------------------------------------------------------------
// Full-round cascade
// ------------------------------------------------------------------

#[test]
fn upkeep_full_round_draws_and_grants_then_pauses_at_mythos() {
    let inv1 = InvestigatorId(1);

    // seat_and_open → seed exhausted asset → mulligan (keep hand).
    let mut r1 = seat_and_open(setup(), &roster());
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        r1.outcome
    );
    // Seed one exhausted asset after seating so we can verify ready-all fires.
    seed_exhausted_asset(&mut r1.state);

    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert!(matches!(r2.outcome, EngineOutcome::AwaitingInput { .. }));

    // Snapshot baselines at end-of-Investigation (before Upkeep runs).
    let res_before = r2.state.investigators[&inv1].resources;
    let hand_before = r2.state.investigators[&inv1].hand.len();
    let round_before = r2.state.round; // should be 1

    // EndTurn: Investigation → Enemy → Upkeep → Mythos, pausing at the
    // step-1.4 encounter-draw prompt (AwaitingInput).
    let r3 = take_turn_action(r2.state, &TurnAction::EndTurn);

    assert!(matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(r3.state.phase, Phase::Mythos, "cascade must land in Mythos");
    assert_eq!(
        r3.state.round,
        round_before + 1,
        "round bumped on Mythos entry"
    );
    assert!(
        r3.state.current_encounter_drawer().is_some(),
        "draw cursor must be seeded"
    );
    assert_eq!(
        r3.state.investigators[&inv1].resources,
        res_before + 1,
        "gained 1 resource during Upkeep"
    );
    assert_eq!(
        r3.state.investigators[&inv1].hand.len(),
        hand_before + 1,
        "drew 1 card during Upkeep"
    );
    assert!(
        !r3.state.investigators[&inv1].cards_in_play[0].exhausted,
        "exhausted asset was readied during Upkeep"
    );
}

// ------------------------------------------------------------------
// Replay determinism
// ------------------------------------------------------------------

#[test]
fn upkeep_round_replay_is_deterministic() {
    // Drive the same sequence twice to verify replay determinism.
    let run_sequence = |initial: GameState| -> GameState {
        let mut r = seat_and_open(initial, &roster());
        seed_exhausted_asset(&mut r.state);
        let state = apply(
            r.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            }),
        )
        .state;
        take_turn_action(state, &TurnAction::EndTurn).state
    };

    let final_state = run_sequence(setup());

    // --- Second pass: replay from the same initial state. ---
    let replayed_state = run_sequence(setup());

    // Replaying the same action log reproduces state bit-for-bit.
    assert_eq!(
        final_state, replayed_state,
        "replay must reproduce identical state"
    );
}
