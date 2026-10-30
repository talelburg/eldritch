//! End-to-end test of the on-draw resolution path, against a real Core Set
//! treachery.
//!
//! Staged **at** the Mythos phase's step-1.4 encounter draw, and driven by
//! confirming that draw, so what is under test is the reveal → Revelation →
//! discard path itself rather than the Mythos phase walk that leads to it
//! (`mythos_phase.rs` owns the phase walk). The test exercises:
//!
//! - Happy path: revealing the treachery emits `Event::CardRevealed`, resolves
//!   its Revelation effect, and discards the card.
//! - Empty-deck reject when both deck and discard are empty.
//!
//! Lives in `crates/cards/tests/` (ADR 0016) because it runs against
//! `cards::TEST_REGISTRIES`: `game-core` cannot reach the corpus by crate
//! direction.
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
//!   draw. Staged from its own printed card rather than hand-stamped onto a
//!   stock location, which is the impersonation ADR 0016 forbids.

use card_dsl::card_data::CardType;
use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::event::Event;
use game_core::state::{
    CardCode, GameStateBuilder, InvestigatorId, LocationId, MythosPhaseFrame, MythosResume, Phase,
};
use game_core::test_support::{location, seat, TestSession};

/// Ancient Evils — *"**Revelation** - Place 1 doom on the current agenda."*
const ANCIENT_EVILS: &str = "01166";
/// Rise of the Ghouls — the board's sole agenda.
const RISE_OF_THE_GHOULS: &str = "01106";
/// The Study — the board's sole location.
const STUDY: &str = "01111";

const INV: InvestigatorId = InvestigatorId(1);

/// The Study's id on the board.
const STUDY_ID: LocationId = LocationId(1);

/// The board both tests start from: Mythos step 1.4 with the investigator at
/// the Study to draw, one agenda — its printed doom threshold read from the
/// corpus rather than transcribed, so a snapshot refresh cannot silently
/// invalidate the "the agenda never advances" premise above — and
/// `encounter_deck` as the encounter deck.
fn drawing(encounter_deck: &[&str]) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(STUDY_ID).as_card(STUDY))
        .with_seat(seat(1).at(STUDY_ID))
        .with_seating([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
        .with_agenda_deck([RISE_OF_THE_GHOULS])
        .with_encounter_deck(encounter_deck.iter().copied())
        .build();
    state.starting_location = Some(STUDY_ID);
    TestSession::new(state, TEST_REGISTRIES)
}

#[test]
fn revealing_ancient_evils_runs_revelation_and_discards() {
    let draw = drawing(&[ANCIENT_EVILS]);
    let pre_doom = draw.state().agenda_doom;
    let pre_deck_len = draw.state().encounter_deck.len();
    assert!(pre_deck_len >= 1, "fixture must seed at least one card");

    // The draw reveals the card, its Revelation resolves without a prompt of
    // its own, and play runs on out of the Mythos phase to the turn.
    let session = draw.confirm();
    session.assert_at_turn_menu(INV);

    // CardRevealed fires for Ancient Evils.
    assert_event!(
        session.events(),
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == INV
                && *code == CardCode::new(ANCIENT_EVILS)
                && *card_type == CardType::Treachery
    );

    let state = session.state();
    // Revelation effect ran: 1 doom on the current agenda. (The synthetic
    // predecessor gained the controller a resource here; the printed clause is
    // doom, so this is the one assertion the flip could not carry over
    // verbatim.)
    assert_eq!(
        state.agenda_doom,
        pre_doom + 1,
        "Revelation should place 1 doom on the current agenda",
    );

    // Card moved deck → discard.
    assert_eq!(
        state.encounter_deck.len(),
        pre_deck_len - 1,
        "deck length should decrement by 1",
    );
    assert!(
        state
            .encounter_discard
            .contains(&CardCode::new(ANCIENT_EVILS)),
        "Ancient Evils should be in discard after Revelation resolves",
    );
}

#[test]
fn rejects_when_encounter_deck_and_discard_both_empty() {
    let draw = drawing(&[]);
    assert!(draw.state().encounter_deck.is_empty());
    assert!(draw.state().encounter_discard.is_empty());
    let seen = draw.events().len();

    let session = draw.confirm();

    let reason = session.expect_rejected();
    assert!(
        reason.contains("encounter deck and discard both empty"),
        "unexpected reject reason: {reason:?}",
    );
    assert_eq!(
        session.events().len(),
        seen,
        "no events should fire on empty-deck reject; got {:?}",
        &session.events()[seen..],
    );
}
