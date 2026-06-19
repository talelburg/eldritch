//! Integration tests for Upkeep phase content.
//!
//! Drives full apply cycles through `StartScenario` → `Mulligan` →
//! `EndTurn`, verifying the Upkeep cascade (ready exhausted cards,
//! draw 1, gain 1 resource, round bump) and replay determinism.
//!
//! Lives in `crates/scenarios/tests/` for the same reasons as
//! `mythos_phase.rs`: process isolation lets us install `TEST_REGISTRY`
//! without colliding with other test binaries, and the crate-dependency
//! direction prevents `game-core` unit tests from using real registries.
//!
//! We install [`TEST_REGISTRY`] but intentionally do **not** install the
//! scenario registry, mirroring `mythos_phase.rs`.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, Phase};
use game_core::{Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::TEST_REGISTRY;
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();

fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

// ------------------------------------------------------------------
// Full-round cascade
// ------------------------------------------------------------------

#[test]
fn upkeep_full_round_draws_and_grants_then_pauses_at_mythos() {
    install_test_registry();

    let mut base = synthetic::setup();
    // Seed 6 cards into the investigator's deck before StartScenario.
    // StartScenario draws 5 for the opening hand, leaving 1 in the deck
    // for the Upkeep draw.
    let inv1 = InvestigatorId(1);
    {
        let inv = base.investigators.get_mut(&inv1).unwrap();
        for i in 0..6u32 {
            inv.deck.push(CardCode::new(format!("01{i:03}")));
        }
        // Seed one exhausted asset so we can verify ready-all fires.
        let mut card = CardInPlay::enter_play(CardCode::new("01010"), CardInstanceId(1));
        card.exhausted = true;
        inv.cards_in_play.push(card);
    }

    // StartScenario → mulligan (keep hand).
    let r1 = apply(
        base,
        Action::Player(PlayerAction::StartScenario { roster: vec![] }),
    );
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "StartScenario opens the mulligan prompt, got {:?}",
        r1.outcome
    );

    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert_eq!(r2.outcome, EngineOutcome::Done);

    // Snapshot baselines at end-of-Investigation (before Upkeep runs).
    let res_before = r2.state.investigators[&inv1].resources;
    let hand_before = r2.state.investigators[&inv1].hand.len();
    let round_before = r2.state.round; // should be 1

    // EndTurn: Investigation → Enemy → Upkeep → Mythos.
    let r3 = apply(r2.state, Action::Player(PlayerAction::EndTurn));

    assert_eq!(r3.outcome, EngineOutcome::Done);
    assert_eq!(r3.state.phase, Phase::Mythos, "cascade must land in Mythos");
    assert_eq!(
        r3.state.round,
        round_before + 1,
        "round bumped on Mythos entry"
    );
    assert!(
        r3.state.mythos_draw_pending.is_some(),
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
    install_test_registry();

    // Build the same initial state as the full-round test.
    let make_initial = || {
        let mut base = synthetic::setup();
        let inv1 = InvestigatorId(1);
        {
            let inv = base.investigators.get_mut(&inv1).unwrap();
            for i in 0..6u32 {
                inv.deck.push(CardCode::new(format!("01{i:03}")));
            }
            let mut card = CardInPlay::enter_play(CardCode::new("01010"), CardInstanceId(1));
            card.exhausted = true;
            inv.cards_in_play.push(card);
        }
        base
    };

    // --- First pass: drive and collect the action log. ---
    let actions = vec![
        Action::Player(PlayerAction::StartScenario { roster: vec![] }),
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
        Action::Player(PlayerAction::EndTurn),
    ];

    let final_state = {
        let mut state = make_initial();
        for action in &actions {
            let result = apply(state, action.clone());
            state = result.state;
        }
        state
    };

    // --- Second pass: replay from the same initial state. ---
    let replayed_state = {
        let mut state = make_initial();
        for action in &actions {
            let result = apply(state, action.clone());
            state = result.state;
        }
        state
    };

    // Replaying the same action log reproduces state bit-for-bit.
    assert_eq!(
        final_state, replayed_state,
        "replay must reproduce identical state"
    );
}
