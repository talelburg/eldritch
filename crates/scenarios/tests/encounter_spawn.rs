//! End-to-end test of the spawn-on-reveal path (#127).
//!
//! Installs the synthetic `TEST_REGISTRY` (same registry used by
//! `encounter_reveal.rs`) so the on-draw path resolves against the
//! synthetic enemy code rather than a real corpus card. The test
//! exercises:
//!
//! - Happy path: revealing the synthetic enemy from the encounter
//!   deck emits `Event::CardRevealed` (kind Enemy), then
//!   `Event::EnemySpawned` at the right location, engaged with the
//!   drawing investigator. The enemy lands in `state.enemies` and
//!   does NOT appear in `encounter_discard`.
//! - Multi-investigator suspend: two investigators at the spawn
//!   location → the spawn suspends (`AwaitingInput`) for the lead
//!   investigator's `PickInvestigator` (#128, option A), leaving the
//!   enemy in play but unengaged until the pick resolves.
//!
//! Default-spawn and location-not-in-play coverage lives in
//! `spawn_enemy_tests` in `dispatch.rs`, where we can construct
//! synth metadata inline.
//!
//! Lives in `crates/scenarios/tests/` because the `cards`-crate
//! dependency direction prevents game-core tests from constructing
//! real card-shaped registries, and because `card_registry::install`
//! is process-global — an integration test binary gets its own
//! process, so this install doesn't collide with `cards::REGISTRY`
//! installs in other test binaries (e.g.
//! `crates/cards/tests/play_card.rs`).

use std::sync::Once;

use game_core::action::EngineRecord;
use game_core::card_data::CardType;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{CardCode, InvestigatorId, LocationId};
use game_core::{assert_event_sequence, Action};
use scenarios::test_fixtures::synth_cards::{SYNTH_ENEMY_CODE, SYNTH_LOC_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();

fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

#[test]
fn revealing_synth_enemy_spawns_at_specific_location_engaged_with_drawer() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Place the drawing investigator at the synth location so the
    // engagement-on-spawn resolves to them.
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    // Replace the seeded treachery on top of the deck with the synth
    // enemy.
    state.encounter_deck.clear();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_ENEMY_CODE.into()));

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);

    // CardRevealed (Enemy) fires first; EnemySpawned follows.
    assert_event_sequence!(
        result.events,
        Event::CardRevealed { card_type, code, .. }
            if *card_type == CardType::Enemy
                && *code == CardCode(SYNTH_ENEMY_CODE.into()),
        Event::EnemySpawned { code, location, engaged_with, .. }
            if *code == CardCode(SYNTH_ENEMY_CODE.into())
                && *location == LocationId(10)
                && *engaged_with == Some(InvestigatorId(1)),
        Event::EnemyEngaged { investigator, .. }
            if *investigator == InvestigatorId(1),
    );

    // Enemy is in play.
    assert_eq!(
        result.state.enemies.len(),
        1,
        "exactly one enemy should be in play after spawn",
    );
    let enemy = result.state.enemies.values().next().unwrap();
    assert_eq!(enemy.current_location, Some(LocationId(10)));
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(1)));

    // Enemy is NOT in encounter_discard (enemies stay in play; only
    // treacheries discard after Revelation).
    assert!(
        !result
            .state
            .encounter_discard
            .contains(&CardCode(SYNTH_ENEMY_CODE.into())),
        "spawned enemy must not appear in encounter_discard",
    );

    // Sanity: the synth location's code is what spawn_enemy looked up.
    let loc = result.state.locations.get(&LocationId(10)).unwrap();
    assert_eq!(loc.code, CardCode(SYNTH_LOC_CODE.into()));
}

#[test]
fn revealing_synth_enemy_with_two_investigators_at_loc_suspends_for_lead_pick() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Add a second investigator at the same location.
    let mut inv2 = game_core::test_support::test_investigator(2);
    inv2.current_location = Some(LocationId(10));
    state.investigators.insert(InvestigatorId(2), inv2);
    state.turn_order.push(InvestigatorId(2));
    // First investigator also at LocationId(10).
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));

    // Swap deck to the synth enemy.
    state.encounter_deck.clear();
    state
        .encounter_deck
        .push_back(CardCode(SYNTH_ENEMY_CODE.into()));

    let result = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
    );

    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "multi-investigator spawn now suspends for the lead's PickInvestigator, got {:?}",
        result.outcome,
    );
    assert!(result.state.spawn_engage_pending.is_some());
    let enemy = result.state.enemies.values().next().expect("enemy placed");
    assert_eq!(
        enemy.engaged_with, None,
        "engagement deferred until the lead picks",
    );
}
