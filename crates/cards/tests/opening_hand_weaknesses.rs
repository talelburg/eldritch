//! #508 acceptance: opening-hand weaknesses are set aside and reshuffled
//! per Rules Reference setup step 8, against a real Core Set weakness.
//!
//! "Each weakness card drawn during this step is ignored, set aside
//! (without resolving it), and replaced by drawing another card from
//! the deck. Upon completion of this step, shuffle each of these
//! weakness cards back into its owner's deck." (RR p.27, Step 8)
//!
//! Lives in `crates/cards/tests/` (ADR 0016) because it installs the real
//! `cards::REGISTRY`: `game-core` cannot reach the corpus by crate direction,
//! and each `tests/*.rs` is its own process, so this install does not collide
//! with the registries other integration binaries claim.
//!
//! The cards, all Core Set, all verified against
//! `data/arkhamdb-snapshot/pack/core/` and their rulings files:
//!
//! - **Cover Up 01007** — a `weakness` subtype whose printed text opens
//!   *"**Revelation** - Put Cover Up into play in your threat area, with 3 clues
//!   on it."* **Step 8 never resolves the card**, so that Revelation and the
//!   `[reaction]` and **Forced** clauses under it are all inert here; what the
//!   test needs from 01007 is only that the corpus marks it a weakness. Its
//!   rulings (<https://arkhamdb.com/card/01007>) are entirely about the
//!   clue-replacement reaction and the game-end trauma — both downstream of a
//!   Revelation this test never reaches. `cover_up.rs` owns those.
//! - **Roland Banks 01001** — the seated investigator, and the one Cover Up's
//!   `restrictions: investigator:01001` names, so the deck below is a legal one.
//!   His *"\[reaction\] After you defeat an enemy: Discover 1 clue at your
//!   location"* has no trigger here; his rulings
//!   (<https://arkhamdb.com/card/01001>) all scope it.
//! - **Study 01111** — the starting location, so `seat_and_open` has somewhere
//!   to place the roster. No printed ability text and no rulings
//!   (`data/arkhamdb-faq/no-rulings.txt`), so nothing on the board reacts to the
//!   opening draw. Built through the engine from its own corpus metadata rather
//!   than hand-stamped onto a `test_location`, which is the impersonation ADR
//!   0016 forbids.
//!
//! **The non-weakness filler is not a card.** Every deck below needs some
//! number of opaque tokens for the weakness to be drawn *among*; the draw and
//! discard paths never look them up, so nothing about them has to be real. They
//! are `_ohw_*`-prefixed codes, per-binary and underscore-led, which no
//! `ArkhamDB` code can collide with. The predecessor of this file padded its
//! decks with `01001`–`01004` — four real investigator cards sitting in a zone
//! none of them can legally occupy, and the purest instance of the impersonation
//! ADR 0016 forbids.

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::seat_and_open;
use game_core::state::{CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{test_investigator, GameStateBuilder};
use game_core::{Action, InputResponse, PlayerAction};

/// Cover Up — the real Core Set weakness this file sets aside.
const COVER_UP: &str = "01007";
/// Roland Banks — the seated investigator, and Cover Up's named owner.
const ROLAND: &str = "01001";
/// The Study — the starting location.
const STUDY: &str = "01111";

/// The opaque non-weakness filler. Not a card, and self-evidently so: the
/// underscore prefix cannot collide with an `ArkhamDB` code, and `ohw` scopes it
/// to this binary. `n` is 1-based.
fn filler(n: u8) -> CardCode {
    CardCode::new(format!("_ohw_filler_{n}"))
}

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

const INV: InvestigatorId = InvestigatorId(1);

// ---- helpers ---------------------------------------------------------------

/// The board the `seat_and_open` tests start from: the Study in play as the
/// starting location and no investigators — callers supply the roster.
fn board() -> GameState {
    let mut state = GameStateBuilder::new().build();
    let study = state.add_location(cards::by_code(STUDY).expect("Study 01111 in corpus"));
    state.starting_location = Some(study);
    state
}

/// Apply a "keep my whole hand" mulligan response (empty `PickMultiple`).
fn keep_hand(state: GameState) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
}

// ---- Test 1: opening-hand weakness is set aside and replaced ---------------

/// A 5-card deck where one card is Cover Up. Because the deck has exactly 5
/// cards and `start_scenario` draws 5, all cards are in hand after the initial
/// draw regardless of shuffle order — the weakness is guaranteed to be drawn.
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
        CardCode::new(COVER_UP), // weakness
        filler(1),
        filler(2),
        filler(3),
        filler(4),
    ];
    let roster = vec![RosterEntry {
        investigator: CardCode::new(ROLAND),
        deck,
    }];

    // seat_and_open → initial draw + weakness set-aside, then mulligan prompt.
    let r1 = seat_and_open(board(), &roster);
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
            if code.as_str() == COVER_UP
        )),
        "WeaknessSetAside must fire for Cover Up during initial draw; events = {:?}",
        r1.events,
    );

    // Hand has no weakness after the initial replace.
    let inv = &r1.state.investigators[&INV];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == COVER_UP),
        "weakness must NOT be in hand after initial draw; hand = {:?}",
        inv.hand,
    );
    // Weakness is in setaside, waiting for drain.
    assert!(
        inv.setaside.iter().any(|c| c.as_str() == COVER_UP),
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
        !inv2.hand.iter().any(|c| c.as_str() == COVER_UP),
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
        inv2.deck.iter().any(|c| c.as_str() == COVER_UP),
        "weakness must be in deck after drain; deck = {:?}",
        inv2.deck,
    );
}

// ---- Test 2: mulligan redraw also avoids weaknesses -----------------------

/// The deck holds only the weakness, so the mulligan redraw is guaranteed to
/// draw it: the filler is set aside first (#637 — a mulliganed card is held out
/// of the deck while its replacement is drawn), leaving the weakness as the only
/// card available.
///
/// The set-aside filler then shuffles back, closing the mulligan, and only then
/// does `replace_opening_hand_weaknesses` run: it sets the weakness aside again
/// and draws its replacement off the restored deck — the filler, which is legal
/// here because this is a step-8 draw, not the mulligan draw. The hand ends
/// weakness-free *and* at its original size; running the sweep before the filler
/// returned would leave the investigator holding nothing. At drain the weakness
/// is shuffled back into the deck.
///
/// No seed derivation is needed: every draw here comes off a one-card deck, and
/// `shuffle_player_deck` no-ops below two cards.
#[test]
fn mulligan_redraw_weakness_is_set_aside() {
    let mut inv = test_investigator(1);
    // Real investigator code so max_health()/max_sanity() read from the
    // installed registry.
    inv.investigator_card.code = CardCode::new(ROLAND);
    // Hand: one non-weakness card to mulligan.
    inv.hand = vec![filler(1)];
    // Deck: only the weakness — guarantees the mulligan redraw draws it.
    inv.deck = vec![CardCode::new(COVER_UP)];

    let state = GameStateBuilder::new()
        .with_rng_seed(42)
        .with_investigator(inv)
        .with_phase(Phase::Investigation)
        .with_turn_order([INV])
        .with_mulligan_remaining([INV])
        .build();

    // Player mulligans index 0 (the filler):
    //   → filler set aside (held out of the deck) → deck = [weakness]
    //   → draw 1 → weakness drawn → hand = [weakness], deck = []
    //   → set-aside filler shuffles back → deck = [filler] (1 card: no-op)
    //   → replace_opening_hand_weaknesses: weakness → setaside, draw 1
    //   → draws the filler → hand = [filler], deck = []
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
            if code.as_str() == COVER_UP
        )),
        "WeaknessSetAside must fire for weakness drawn during mulligan; events = {:?}",
        r.events,
    );

    let inv = &r.state.investigators[&INV];

    // Hand has no weakness.
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == COVER_UP),
        "weakness must NOT be in hand after mulligan; hand = {:?}",
        inv.hand,
    );

    // ...and the mulligan did not cost the investigator a card. This is what
    // pins the step-8 sweep to *after* the set-aside cards return: run it
    // before, and the sweep's replacement draw finds an empty deck and the
    // hand ends empty.
    assert_eq!(
        inv.hand,
        vec![filler(1)],
        "mulligan must leave the hand at its original size; hand = {:?}",
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
        inv.deck.iter().any(|c| c.as_str() == COVER_UP),
        "weakness must be in deck after drain; deck = {:?}",
        inv.deck,
    );
}

// ---- Test 3: non-weakness deck unchanged ----------------------------------

/// A deck with no weakness cards must produce no `WeaknessSetAside` events
/// and leave the hand intact (5 non-weakness cards drawn).
#[test]
fn non_weakness_deck_produces_no_weakness_events() {
    let deck: Vec<CardCode> = (1u8..=5).map(filler).collect();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(ROLAND),
        deck,
    }];

    let r1 = seat_and_open(board(), &roster);
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
