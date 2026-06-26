//! Integration tests for Upkeep phase content.
//!
//! Drives full apply cycles through scenario setup (via `seat_and_open`) →
//! `Mulligan` → `EndTurn`, verifying the Upkeep cascade (ready exhausted cards,
//! draw 1, gain 1 resource, round bump) and replay determinism.
//!
//! Lives in `crates/scenarios/tests/` for the same reasons as
//! `mythos_phase.rs`: process isolation lets us install `TEST_REGISTRY`
//! without colliding with other test binaries, and the crate-dependency
//! direction prevents `game-core` unit tests from using real registries.
//!
//! We install [`TEST_REGISTRY`] but intentionally do **not** install the
//! scenario registry, mirroring `mythos_phase.rs`.

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome};
use game_core::seat_and_open;
use game_core::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, Phase};
use game_core::test_support::{take_turn_action, TEST_INV};
use game_core::{Action, InputResponse, PlayerAction, TurnAction};
use scenarios::test_fixtures::synth_cards::TEST_REGISTRY;
use scenarios::test_fixtures::synthetic;

#[ctor::ctor]
fn install_test_registry() {
    let _ = game_core::card_registry::install(TEST_REGISTRY);
}

// ------------------------------------------------------------------
// Full-round cascade
// ------------------------------------------------------------------

#[test]
fn upkeep_full_round_draws_and_grants_then_pauses_at_mythos() {
    let inv1 = InvestigatorId(1);
    // Seed 6 cards via the roster: seat_and_open draws 5 for the opening
    // hand, leaving 1 in the deck for the Upkeep draw.
    let deck = (0..6u32)
        .map(|i| CardCode::new(format!("01{i:03}")))
        .collect();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck,
    }];

    // seat_and_open → seed exhausted asset → mulligan (keep hand).
    let mut r1 = seat_and_open(synthetic::setup(), &roster);
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        r1.outcome
    );
    // Seed one exhausted asset after seating so we can verify ready-all fires.
    {
        let inv = r1.state.investigators.get_mut(&inv1).unwrap();
        let mut card = CardInPlay::enter_play(CardCode::new("01010"), CardInstanceId(1));
        card.exhausted = true;
        inv.cards_in_play.push(card);
    }

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
    let inv1 = InvestigatorId(1);
    // 6 deck cards: seat_and_open draws 5, leaving 1 for the upkeep draw.
    let deck: Vec<CardCode> = (0..6u32)
        .map(|i| CardCode::new(format!("01{i:03}")))
        .collect();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck,
    }];

    // Drive the same sequence twice to verify replay determinism.
    let run_sequence = |initial: game_core::state::GameState| -> game_core::state::GameState {
        let mut r = seat_and_open(initial, &roster);
        {
            let inv = r.state.investigators.get_mut(&inv1).unwrap();
            let mut card = CardInPlay::enter_play(CardCode::new("01010"), CardInstanceId(1));
            card.exhausted = true;
            inv.cards_in_play.push(card);
        }
        let state = apply(
            r.state,
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected: vec![] },
            }),
        )
        .state;
        take_turn_action(state, &TurnAction::EndTurn).state
    };

    let final_state = run_sequence(synthetic::setup());

    // --- Second pass: replay from the same initial state. ---
    let replayed_state = run_sequence(synthetic::setup());

    // Replaying the same action log reproduces state bit-for-bit.
    assert_eq!(
        final_state, replayed_state,
        "replay must reproduce identical state"
    );
}
