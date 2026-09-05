//! End-to-end test of the on-draw resolution path, against a real Core Set
//! treachery.
//!
//! Drives [`EngineRecord::EncounterCardRevealed`] **directly** — not through a
//! player action — so what is under test is the reveal → Revelation → discard
//! path itself rather than the Mythos phase that normally calls it
//! (`mythos_phase.rs` owns the phase walk). The test exercises:
//!
//! - Happy path: revealing the treachery emits `Event::CardRevealed`, resolves
//!   its Revelation effect, and discards the card.
//! - Empty-deck reject when both deck and discard are empty.
//!
//! Lives in `crates/cards/tests/` (ADR 0016) because it installs the real
//! `cards::REGISTRY`: `game-core` cannot reach the corpus by crate direction,
//! and each `tests/*.rs` is its own process, so this install does not collide
//! with the registries other integration binaries claim.
//!
//! The cards, all Core Set, all verified against
//! `data/arkhamdb-snapshot/pack/core/` and their rulings files:
//!
//! - **Ancient Evils 01166** — *"**Revelation** - Place 1 doom on the current
//!   agenda. This effect can cause the current agenda to advance."* The
//!   Revelation resolves inline (no skill test, no choice) and observably moves
//!   `agenda_doom`, so a reveal that silently skipped the Revelation would not
//!   pass. No rulings (`data/arkhamdb-faq/no-rulings.txt`).
//! - **Rise of the Ghouls 01106** — the board's sole agenda, so the Revelation's
//!   doom lands somewhere observable. Its printed threshold is read from the
//!   corpus rather than transcribed, and is far above the 1 doom placed here, so
//!   the agenda never advances.
//! - **Study 01111** — a location with no printed ability text and no rulings
//!   (`data/arkhamdb-faq/no-rulings.txt`), so nothing on the board reacts to the
//!   draw. Built through the engine from its own corpus metadata rather than
//!   hand-stamped onto a `test_location`, which is the impersonation ADR 0016
//!   forbids.
//! - **Roland Banks 01001** — the seated investigator, so `max_health()` /
//!   `max_sanity()` resolve against the installed registry. His *"\[reaction\]
//!   After you defeat an enemy: Discover 1 clue at your location"* has no
//!   trigger here. Rulings (<https://arkhamdb.com/card/01001>) all scope that
//!   reaction and his deckbuilding.

use game_core::action::EngineRecord;
use game_core::card_data::{CardKind, CardType};
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{Agenda, CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{test_investigator, GameStateBuilder};
use game_core::{assert_event, Action};

/// Ancient Evils — *"**Revelation** - Place 1 doom on the current agenda."*
const ANCIENT_EVILS: &str = "01166";
/// Rise of the Ghouls — the board's sole agenda.
const RISE_OF_THE_GHOULS: &str = "01106";
/// The Study — the board's sole location.
const STUDY: &str = "01111";
/// Roland Banks — the seated investigator.
const ROLAND: &str = "01001";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Rise of the Ghouls' printed doom threshold, read from the corpus rather than
/// transcribed, so a snapshot refresh cannot silently invalidate the "the agenda
/// never advances" premise above.
fn agenda_doom_threshold(code: &str) -> u8 {
    match cards::by_code(code).expect("agenda in corpus").kind {
        CardKind::Agenda { doom_threshold } => doom_threshold,
        ref kind => panic!("{code} is not an Agenda ({kind:?})"),
    }
}

/// The board both tests start from: Roland at the Study, one agenda, and one
/// Ancient Evils on the encounter deck. The investigator is seated by hand
/// rather than via `seat_and_open` because these tests drive
/// [`EngineRecord::EncounterCardRevealed`] directly.
fn board() -> GameState {
    let mut state = GameStateBuilder::new().build();
    let study = state.add_location(cards::by_code(STUDY).expect("Study 01111 in corpus"));
    state.starting_location = Some(study);
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(RISE_OF_THE_GHOULS),
        doom_threshold: agenda_doom_threshold(RISE_OF_THE_GHOULS),
    }];
    let mut inv = test_investigator(1);
    inv.investigator_card.code = CardCode::new(ROLAND);
    inv.current_location = Some(study);
    state.investigators.insert(InvestigatorId(1), inv);
    state.turn_order = vec![InvestigatorId(1)];
    state.active_investigator = Some(InvestigatorId(1));
    state.phase = Phase::Mythos;
    state.encounter_deck.push_back(CardCode::new(ANCIENT_EVILS));
    state
}

#[test]
fn revealing_ancient_evils_runs_revelation_and_discards() {
    let inv1 = InvestigatorId(1);
    let state = board();
    let pre_doom = state.agenda_doom;
    let pre_deck_len = state.encounter_deck.len();
    assert!(pre_deck_len >= 1, "fixture must seed at least one card");

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: inv1 }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);

    // CardRevealed fires for Ancient Evils.
    assert_event!(
        result.events,
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == inv1
                && *code == CardCode::new(ANCIENT_EVILS)
                && *card_type == CardType::Treachery
    );

    // Revelation effect ran: 1 doom on the current agenda. (The synthetic
    // predecessor gained the controller a resource here; the printed clause is
    // doom, so this is the one assertion the flip could not carry over
    // verbatim.)
    assert_eq!(
        result.state.agenda_doom,
        pre_doom + 1,
        "Revelation should place 1 doom on the current agenda",
    );

    // Card moved deck → discard.
    assert_eq!(
        result.state.encounter_deck.len(),
        pre_deck_len - 1,
        "deck length should decrement by 1",
    );
    assert!(
        result
            .state
            .encounter_discard
            .contains(&CardCode::new(ANCIENT_EVILS)),
        "Ancient Evils should be in discard after Revelation resolves",
    );
}

#[test]
fn rejects_when_encounter_deck_and_discard_both_empty() {
    let inv1 = InvestigatorId(1);
    let mut state = board();
    // Drain the deck (and ensure discard stays empty).
    state.encounter_deck.clear();
    assert!(state.encounter_discard.is_empty());

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed { investigator: inv1 }),
    );

    match result.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("encounter deck and discard both empty"),
                "unexpected reject reason: {reason:?}",
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(
        result.events.is_empty(),
        "no events should fire on empty-deck reject; got {:?}",
        result.events,
    );
}
