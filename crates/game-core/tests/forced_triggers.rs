//! End-to-end `fire_forced_triggers` flow with a mock `CardRegistry`
//! covering a single `EventPattern::EnteredLocation` forced ability.
//!
//! Lives at `crates/game-core/tests/` (a separate integration-test
//! binary, hence its own process and its own `OnceLock<CardRegistry>`)
//! so installing a mock registry here doesn't collide with game-core's
//! in-crate tests or with `card_registry::tests::install_is_idempotent`.
//! Mirrors `activate_ability.rs` / `on_skill_test_resolution.rs`.
//!
//! No real card carries `EventPattern::EnteredLocation` yet — the
//! first consumer will land when a scenario-structure card with a
//! location-entry forced ability is implemented. Until then, mock
//! cards are the only way to exercise the full path.

use std::sync::OnceLock;

use game_core::assert_event;
use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    deal_horror, on_event, Ability, EventPattern, EventTiming, InvestigatorTarget,
};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, LocationId};
use game_core::test_support::{
    fire_forced_at, test_investigator, test_location, ForcedTriggerPoint, TestGame,
};

/// Mock location code: one `EventPattern::EnteredLocation` forced ability
/// that deals 1 horror to the entering investigator.
const HORROR_ATTIC: &str = "test-attic";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == HORROR_ATTIC {
        Some(vec![on_event(
            EventPattern::EnteredLocation,
            EventTiming::After,
            deal_horror(InvestigatorTarget::Controller, 1),
        )])
    } else {
        None
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
        });
    });
}

#[test]
fn forced_on_enter_resolves_immediately() {
    install_mock_registry();

    let mut loc = test_location(10, "Attic");
    loc.code = CardCode(HORROR_ATTIC.into());

    let mut state = TestGame::new()
        .with_investigator_at(test_investigator(1), LocationId(10))
        .with_location(loc)
        .with_active_investigator(InvestigatorId(1))
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_at(
        &mut state,
        &mut events,
        ForcedTriggerPoint::EnteredLocation {
            investigator: InvestigatorId(1),
            location: LocationId(10),
        },
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn forced_on_enter_no_op_when_location_has_no_abilities() {
    install_mock_registry();

    // "plain-loc" is not HORROR_ATTIC — mock registry returns None.
    let mut loc = test_location(10, "Plain Room");
    loc.code = CardCode("plain-loc".into());

    let mut state = TestGame::new()
        .with_investigator_at(test_investigator(1), LocationId(10))
        .with_location(loc)
        .with_active_investigator(InvestigatorId(1))
        .build();

    let mut events = Vec::new();
    let outcome = fire_forced_at(
        &mut state,
        &mut events,
        ForcedTriggerPoint::EnteredLocation {
            investigator: InvestigatorId(1),
            location: LocationId(10),
        },
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror, 0);
    assert!(
        events.is_empty(),
        "no events for a location with no forced abilities"
    );
}
