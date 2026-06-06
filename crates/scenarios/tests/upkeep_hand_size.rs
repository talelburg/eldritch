//! #111 acceptance: upkeep step 4.5 discards down to the hand-size cap.
//!
//! Drives a full apply cycle through `StartScenario` → `Mulligan` →
//! `EndTurn`, padding the sole investigator's hand so that — after the
//! step-4.4 draw — they hold more than the cap at step 4.5. The
//! round-ending `EndTurn` must cascade into upkeep and pause with
//! `AwaitingInput`; resolving the prompt with `DiscardCards` must land
//! the hand at exactly the cap and let the round proceed.
//!
//! Lives in `crates/scenarios/tests/` for the same process-isolation /
//! crate-direction reasons as `upkeep_phase.rs` and `mythos_phase.rs`:
//! we install [`TEST_REGISTRY`] without colliding with other test
//! binaries, and `game-core` unit tests can't reach a real registry.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::state::{CardCode, InvestigatorId, Phase};
use game_core::{Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::TEST_REGISTRY;
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();

fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

/// The hand-size cap enforced at upkeep step 4.5.
const HAND_SIZE_LIMIT: usize = 8;

#[test]
fn upkeep_prompts_and_discards_down_to_eight() {
    install_test_registry();

    let mut base = synthetic::setup();
    let inv1 = InvestigatorId(1);
    {
        let inv = base.investigators.get_mut(&inv1).unwrap();
        // Seed 6 cards into the deck: StartScenario draws 5 for the
        // opening hand, leaving 1 for the step-4.4 upkeep draw.
        for i in 0..6u32 {
            inv.deck.push(CardCode::new(format!("01{i:03}")));
        }
    }

    // StartScenario → mulligan (keep hand).
    let r1 = apply(base, Action::Player(PlayerAction::StartScenario));
    assert_eq!(r1.outcome, EngineOutcome::Done);

    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::Mulligan {
            investigator: inv1,
            indices_to_redraw: vec![],
        }),
    );
    assert_eq!(r2.outcome, EngineOutcome::Done);

    // Pad the hand so we're over cap at 4.5. The step-4.4 draw adds one
    // card; padding to 11 here lands us at 12 cards at the 4.5 check,
    // requiring a discard of 12 - 8 = 4.
    let mut state = r2.state;
    {
        let inv = state.investigators.get_mut(&inv1).unwrap();
        while inv.hand.len() < 11 {
            inv.hand.push(CardCode::new("01999"));
        }
    }
    let hand_before_end = state.investigators[&inv1].hand.len();
    let discard_pile_before = state.investigators[&inv1].discard.len();
    assert!(
        state.hand_size_discard_pending.is_none(),
        "no discard should be pending before the round-ending EndTurn"
    );

    // Act 1: the round-ending EndTurn cascades Investigation → Enemy →
    // Upkeep (4.2 reset, 4.3 ready, 4.4 draw +1, 4.5 hand-size check).
    // The +1 draw pushes the hand to 12 (> cap), so 4.5 suspends.
    let r3 = apply(state, Action::Player(PlayerAction::EndTurn));

    assert!(
        matches!(r3.outcome, EngineOutcome::AwaitingInput { .. }),
        "upkeep 4.5 must prompt for a discard, got {:?}",
        r3.outcome,
    );
    assert!(
        r3.state.hand_size_discard_pending.is_some(),
        "hand_size_discard_pending must be set while awaiting the discard"
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

    // Act 2: submit DiscardCards with exactly (hand_len - cap) indices.
    let discard_count = hand_at_check - HAND_SIZE_LIMIT;
    let indices: Vec<u32> = (0..u32::try_from(discard_count).unwrap()).collect();
    let r4 = apply(
        r3.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::DiscardCards { indices },
        }),
    );

    assert_eq!(
        r4.outcome,
        EngineOutcome::Done,
        "resolving the discard must complete the upkeep cascade"
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
        r4.state.hand_size_discard_pending.is_none(),
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
        r4.state.mythos_draw_pending.is_some(),
        "Mythos draw cursor must be seeded once the round proceeds"
    );
}
