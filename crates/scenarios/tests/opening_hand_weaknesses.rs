//! #508 acceptance: opening-hand weaknesses are set aside and reshuffled
//! per Rules Reference setup step 8.
//!
//! "Each weakness card drawn during this step is ignored, set aside
//! (without resolving it), and replaced by drawing another card from
//! the deck. Upon completion of this step, shuffle each of these
//! weakness cards back into its owner's deck." (RR p.27, Step 8)
//!
//! Lives in `crates/scenarios/tests/` (own process) so it can install
//! [`TEST_REGISTRY`] without colliding with other test binaries.

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::seat_and_open;
use game_core::state::{CardCode, InvestigatorId, Phase};
use game_core::test_support::{test_investigator, GameStateBuilder, TEST_INV};
use game_core::{Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::{SYNTH_COVER_UP_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

#[ctor::ctor(unsafe)]
fn install_test_registry() {
    let _ = game_core::card_registry::install(TEST_REGISTRY);
}

const INV: InvestigatorId = InvestigatorId(1);

// ---- helpers ---------------------------------------------------------------

/// Apply a "keep my whole hand" mulligan response (empty `PickMultiple`).
fn keep_hand(state: game_core::state::GameState) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
}

// ---- Test 1: opening-hand weakness is set aside and replaced ---------------

/// A 5-card deck where one card is the synthetic weakness. Because the deck
/// has exactly 5 cards and `start_scenario` draws 5, all cards are in hand
/// after the initial draw regardless of shuffle order — the weakness is
/// guaranteed to be drawn.
///
/// After `replace_opening_hand_weaknesses`:
/// - The weakness is in `setaside`, not in `hand`.
/// - `WeaknessSetAside` fires.
/// - Hand holds the 4 non-weakness cards.
///
/// After the mulligan keep + drain:
/// - `setaside` is empty.
/// - Weakness is back in `deck`.
#[test]
fn opening_hand_weakness_set_aside_and_returned_to_deck() {
    let deck = vec![
        CardCode::new(SYNTH_COVER_UP_CODE), // weakness
        CardCode::new("01001"),
        CardCode::new("01002"),
        CardCode::new("01003"),
        CardCode::new("01004"),
    ];
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck,
    }];

    // seat_and_open → initial draw + weakness set-aside, then mulligan prompt.
    let r1 = seat_and_open(synthetic::setup(), &roster);
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt, got {:?}",
        r1.outcome,
    );

    // The opening-hand weakness-set-aside events fire during seat_and_open.
    assert!(
        r1.events.iter().any(|e| matches!(
            e,
            Event::WeaknessSetAside { investigator: INV, code }
            if code.as_str() == SYNTH_COVER_UP_CODE
        )),
        "WeaknessSetAside must fire for Cover Up during initial draw; events = {:?}",
        r1.events,
    );

    // Hand has no weakness after the initial replace.
    let inv = &r1.state.investigators[&INV];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == SYNTH_COVER_UP_CODE),
        "weakness must NOT be in hand after initial draw; hand = {:?}",
        inv.hand,
    );
    // Weakness is in setaside, waiting for drain.
    assert!(
        inv.setaside
            .iter()
            .any(|c| c.as_str() == SYNTH_COVER_UP_CODE),
        "weakness must be in setaside before mulligan drains; setaside = {:?}",
        inv.setaside,
    );

    // Keep-hand mulligan: investigator keeps the remaining 4 non-weakness cards.
    // At drain, setaside weaknesses are shuffled back into the deck.
    let r2 = keep_hand(r1.state);
    assert!(
        !matches!(r2.outcome, EngineOutcome::Rejected { .. }),
        "keep-hand mulligan must not reject; outcome = {:?}",
        r2.outcome,
    );

    let inv2 = &r2.state.investigators[&INV];

    // Hand has no weakness after drain.
    assert!(
        !inv2.hand.iter().any(|c| c.as_str() == SYNTH_COVER_UP_CODE),
        "weakness must NOT be in hand after mulligan + drain; hand = {:?}",
        inv2.hand,
    );

    // setaside is clear — weaknesses were moved back to deck.
    assert!(
        inv2.setaside.is_empty(),
        "setaside must be empty after mulligan loop drains; setaside = {:?}",
        inv2.setaside,
    );

    // Weakness is now in the deck (shuffled back per RR step 8).
    assert!(
        inv2.deck.iter().any(|c| c.as_str() == SYNTH_COVER_UP_CODE),
        "weakness must be in deck after drain; deck = {:?}",
        inv2.deck,
    );
}

// ---- Test 2: mulligan redraw also avoids weaknesses -----------------------

/// With seed 42, Fisher-Yates on a 2-element deck [weakness, non1] is a
/// no-op (j = `next_index(2)` = 1, which swaps an element with itself).
/// After the mulligan returns non1 to the deck and shuffles, the deck
/// remains [weakness, non1], so the redraw draws the weakness first.
///
/// `replace_opening_hand_weaknesses` then sets the weakness aside again
/// and draws non1 as the replacement, leaving a weakness-free hand.
/// At drain the weakness is shuffled back into the deck.
///
/// Seed derivation: frozen contract `RngState::new(42)`, first `next_u64`
/// = `0xae90_bfb5_395d_5ba1` (odd) → `% 2 = 1` → i=1, j=1, no swap.
#[test]
fn mulligan_redraw_weakness_is_set_aside() {
    let mut inv = test_investigator(1);
    // Hand: one non-weakness card to mulligan.
    inv.hand = vec![CardCode::new("01001")];
    // Deck: only the weakness — guarantees the mulligan redraw draws it.
    inv.deck = vec![CardCode::new(SYNTH_COVER_UP_CODE)];

    let state = GameStateBuilder::new()
        .with_rng_seed(42)
        .with_investigator(inv)
        .with_phase(Phase::Investigation)
        .with_turn_order([INV])
        .with_mulligan_remaining([INV])
        .build();

    // Player mulligans index 0 ("01001"):
    //   → "01001" pushed to deck → deck = [weakness, "01001"]
    //   → Fisher-Yates(seed=42): j=1, no-op → deck = [weakness, "01001"]
    //   → draw 1 → weakness drawn → hand = [weakness]
    //   → replace_opening_hand_weaknesses: weakness → setaside, draw 1
    //   → deck = ["01001"], draw "01001" → hand = ["01001"]
    //   → deck empty, break.
    // MulliganPerformed{redrawn_count:1}.
    // Drain: setaside[weakness] → deck, shuffle.
    let r = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: vec![game_core::engine::OptionId(0)],
            },
        }),
    );
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "mulligan must not reject; outcome = {:?}",
        r.outcome,
    );

    // WeaknessSetAside fired for the weakness drawn during the mulligan redraw.
    assert!(
        r.events.iter().any(|e| matches!(
            e,
            Event::WeaknessSetAside { investigator: INV, code }
            if code.as_str() == SYNTH_COVER_UP_CODE
        )),
        "WeaknessSetAside must fire for weakness drawn during mulligan; events = {:?}",
        r.events,
    );

    let inv = &r.state.investigators[&INV];

    // Hand has no weakness.
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == SYNTH_COVER_UP_CODE),
        "weakness must NOT be in hand after mulligan; hand = {:?}",
        inv.hand,
    );

    // setaside is clear (drained at mulligan loop completion).
    assert!(
        inv.setaside.is_empty(),
        "setaside must be empty after mulligan drains; setaside = {:?}",
        inv.setaside,
    );

    // Weakness is back in deck.
    assert!(
        inv.deck.iter().any(|c| c.as_str() == SYNTH_COVER_UP_CODE),
        "weakness must be in deck after drain; deck = {:?}",
        inv.deck,
    );
}

// ---- Test 3: non-weakness deck unchanged ----------------------------------

/// A deck with no weakness cards must produce no `WeaknessSetAside` events
/// and leave the hand intact (5 non-weakness cards drawn).
#[test]
fn non_weakness_deck_produces_no_weakness_events() {
    let deck: Vec<CardCode> = (1u32..=5)
        .map(|i| CardCode::new(format!("010{i:02}")))
        .collect();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck,
    }];

    let r1 = seat_and_open(synthetic::setup(), &roster);
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "seat_and_open opens the mulligan prompt",
    );

    // No WeaknessSetAside events during initial draw.
    assert!(
        !r1.events
            .iter()
            .any(|e| matches!(e, Event::WeaknessSetAside { .. })),
        "no WeaknessSetAside must fire for a weakness-free deck; events = {:?}",
        r1.events,
    );

    // All 5 non-weakness cards are in hand.
    let inv = &r1.state.investigators[&INV];
    assert_eq!(
        inv.hand.len(),
        5,
        "hand must hold all 5 non-weakness cards; hand = {:?}",
        inv.hand,
    );

    // Keep mulligan — no WeaknessSetAside during drain either.
    let r2 = keep_hand(r1.state);
    assert!(
        !r2.events
            .iter()
            .any(|e| matches!(e, Event::WeaknessSetAside { .. })),
        "no WeaknessSetAside must fire during mulligan for a weakness-free deck",
    );

    let inv2 = &r2.state.investigators[&INV];
    assert!(
        inv2.setaside.is_empty(),
        "setaside must stay empty for a weakness-free deck",
    );
}
