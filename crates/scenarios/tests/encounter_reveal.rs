//! End-to-end test of the on-draw resolution path.
//!
//! Installs the synthetic `TEST_REGISTRY` (NOT the real
//! `cards::REGISTRY`) so the on-draw path resolves against the
//! synthetic treachery code rather than depending on a real corpus
//! card. The `cards` crate is still compiled in as a workspace dep —
//! what `TEST_REGISTRY` isolates is the runtime registry lookup, not
//! the compile-time footprint. The test exercises:
//!
//! - Happy path: revealing the synthetic treachery emits
//!   `Event::CardRevealed`, resolves its Revelation effect
//!   (gain 1 resource), and discards the card.
//! - Empty-deck reject when both deck and discard are empty.
//!
//! Lives in `crates/scenarios/tests/` (not `game-core/src/engine/`)
//! because the `cards`-crate dependency direction prevents game-core
//! tests from constructing real card-shaped registries, and because
//! `card_registry::install` is process-global — an integration test
//! binary gets its own process, so this install doesn't collide
//! with `cards::REGISTRY` installs in other test binaries (e.g.
//! `crates/cards/tests/play_card.rs`).

use std::sync::Once;

use game_core::action::EngineRecord;
use game_core::card_data::CardType;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId};
use game_core::{assert_event, Action};
use scenarios::test_fixtures::synth_cards::{SYNTH_TREACHERY_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();

fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

#[test]
fn revealing_synth_treachery_runs_revelation_and_discards() {
    install_test_registry();
    let state = synthetic::setup();
    let pre_resources = state.investigators[&InvestigatorId(1)].resources;
    let pre_deck_len = state.encounter_deck.len();
    assert!(pre_deck_len >= 1, "fixture must seed at least one card");

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);

    // CardRevealed fires for the synthetic treachery.
    assert_event!(
        result.events,
        Event::CardRevealed { investigator, code, card_type }
            if *investigator == InvestigatorId(1)
                && *code == CardCode(SYNTH_TREACHERY_CODE.into())
                && *card_type == CardType::Treachery
    );

    // Revelation effect ran: controller gained 1 resource.
    let post_resources = result.state.investigators[&InvestigatorId(1)].resources;
    assert_eq!(
        post_resources,
        pre_resources + 1,
        "Revelation should grant 1 resource",
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
            .contains(&CardCode(SYNTH_TREACHERY_CODE.into())),
        "synth treachery should be in discard after Revelation resolves",
    );
}

#[test]
fn rejects_when_encounter_deck_and_discard_both_empty() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Drain the deck (and ensure discard stays empty).
    state.encounter_deck.clear();
    assert!(state.encounter_discard.is_empty());

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
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
