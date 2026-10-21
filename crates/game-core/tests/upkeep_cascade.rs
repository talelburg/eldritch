//! The Upkeep cascade, driven end to end: ready exhausted cards, draw 1, gain 1
//! resource, bump the round — and replay it identically.
//!
//! Nothing here depends on a card: what the tests needed was a *scenario module*, and under
//! ADR 0016 that is a locally-built shell rather than a shared fixture. The
//! `setup()` below is that shell's state, composed with `GameStateBuilder`.
//!
//! The registries name no scenario module — the resolution hook treats `None`
//! as "no scenario behavior wired up; skip", which is what these tests want: the round
//! must not resolve out from under the cascade.

use std::sync::LazyLock;

use game_core::action::RosterEntry;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    Act, Agenda, CardCode, CardInPlay, ChaosBag, ChaosToken, GameState, GameStateBuilder,
    InvestigatorId, LocationId, Owner, Phase,
};
use game_core::test_support::{self, MockRegistry, TestSession, TEST_INV};

/// Per-binary code prefix. None of these codes is looked up — the deck, the
/// hand and the readied asset are opaque tokens to every step the cascade runs —
/// so they are deliberately not ArkhamDB-shaped (ADR 0016).
const PREFIX: &str = "_uc_";

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        // No probe cards. `build` composes `metadata_for_test_inv` (capacity reads
        // for `TEST_INV`) and `abilities_for_terminal` (the terminal act/agenda
        // reverses that `setup` seeds), which is everything the cascade touches.
        cards: MockRegistry::new().build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// One deck card, distinct per index.
fn deck_code(i: u32) -> CardCode {
    CardCode::new(format!("{PREFIX}deck_{i}"))
}

/// The locally-built scenario shell's state: one revealed location to seat onto,
/// a chaos bag so a skill test could be taken, and two-card act/agenda decks
/// ending in a terminal card. Phase = Mythos, round = 0 — ready for
/// [`engine::seat_and_open`], with no investigator pre-seated (callers pass a roster).
///
/// The thresholds are set high enough that a single round's doom placement never
/// advances the terminal agenda: these tests are about Upkeep, and a scenario
/// that ends mid-cascade would test something else.
fn setup() -> GameState {
    let mut location = test_support::test_location(10, "Upkeep Location");
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
/// opening hand, leaving 1 in the deck for the Upkeep step-4.4 draw.
fn roster() -> Vec<RosterEntry> {
    vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: (0..6u32).map(deck_code).collect(),
        random_basic_weaknesses: 0,
    }]
}

const INV: InvestigatorId = InvestigatorId(1);

/// Seat the roster with `seat_and_open`, put one exhausted asset into the
/// seated investigator's play area — so the step-4.3 ready-all has something
/// to ready — and close the mulligan keeping the hand: Investigation, round 1,
/// at the turn menu.
fn opened_with_an_exhausted_asset() -> TestSession {
    let opened = game_core::engine::seat_and_open(setup(), &roster(), registries());
    // The roster seats the investigator, so the asset goes onto the seated
    // board, with an id minted from the state's counter.
    let mut state = opened.state;
    let id = state.card_instance_ids.mint();
    let mut card = CardInPlay::enter_play(
        CardCode::new(format!("{PREFIX}asset")),
        id,
        Owner::Investigator(INV),
    );
    card.exhausted = true;
    state
        .investigators
        .get_mut(&INV)
        .unwrap()
        .cards_in_play
        .push(card);
    let session = TestSession::new(state, registries()).choose_from_hand(&[]);
    session.assert_at_turn_menu(INV);
    session
}

// ------------------------------------------------------------------
// Full-round cascade
// ------------------------------------------------------------------

#[test]
fn upkeep_full_round_draws_and_grants_then_pauses_at_mythos() {
    let opened = opened_with_an_exhausted_asset();

    // Snapshot baselines at end-of-Investigation (before Upkeep runs).
    let res_before = opened.state().investigators[&INV].resources;
    let hand_before = opened.state().investigators[&INV].hand.len();
    let round_before = opened.state().round; // should be 1

    // EndTurn: Investigation → Enemy → Upkeep → Mythos, pausing at the
    // step-1.4 encounter-draw prompt.
    let session = opened.take(&TurnAction::EndTurn);
    let state = session.state();

    session.assert_at_encounter_draw();
    assert_eq!(state.phase, Phase::Mythos, "cascade must land in Mythos");
    assert_eq!(
        state.round,
        round_before + 1,
        "round bumped on Mythos entry"
    );
    assert!(
        state.current_encounter_drawer().is_some(),
        "draw cursor must be seeded"
    );
    assert_eq!(
        state.investigators[&INV].resources,
        res_before + 1,
        "gained 1 resource during Upkeep"
    );
    assert_eq!(
        state.investigators[&INV].hand.len(),
        hand_before + 1,
        "drew 1 card during Upkeep"
    );
    assert!(
        !state.investigators[&INV].cards_in_play[0].exhausted,
        "exhausted asset was readied during Upkeep"
    );
}

// ------------------------------------------------------------------
// Replay determinism
// ------------------------------------------------------------------

#[test]
fn upkeep_round_replay_is_deterministic() {
    // Drive the same sequence twice to verify replay determinism.
    let run_sequence = || -> GameState {
        opened_with_an_exhausted_asset()
            .take(&TurnAction::EndTurn)
            .state()
            .clone()
    };

    let final_state = run_sequence();

    // --- Second pass: replay from the same initial state. ---
    let replayed_state = run_sequence();

    // Replaying the same action log reproduces state bit-for-bit.
    assert_eq!(
        final_state, replayed_state,
        "replay must reproduce identical state"
    );
}
