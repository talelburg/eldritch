//! #128 integration: spawn-engagement tie resolved through the real
//! registry + Mythos draw path (option A), plus hunter-movement replay
//! equality across a `PickLocation` round-trip.

use std::sync::Once;

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{apply, EngineOutcome};
use game_core::state::{EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{test_enemy, test_investigator, test_location, TestGame};
use game_core::Action;
use scenarios::test_fixtures::synth_cards::{SYNTH_ENEMY_CODE, TEST_REGISTRY};
use scenarios::test_fixtures::synthetic;

static INSTALL: Once = Once::new();
fn install_test_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

#[test]
fn multi_investigator_spawn_engagement_resolves_via_lead_pick() {
    install_test_registry();
    let mut state = synthetic::setup();
    // Second investigator co-located at the synth spawn location (10).
    let mut inv2 = test_investigator(2);
    inv2.current_location = Some(LocationId(10));
    state.investigators.insert(InvestigatorId(2), inv2);
    state.turn_order.push(InvestigatorId(2));
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(10));
    // Drive through the real Mythos draw path.
    state.phase = Phase::Mythos;
    state.mythos_draw_pending = Some(InvestigatorId(1));
    state.encounter_deck.clear();
    state
        .encounter_deck
        .push_back(game_core::state::CardCode(SYNTH_ENEMY_CODE.into()));

    // 1) Drawing the enemy suspends for the lead's PickInvestigator.
    let r1 = apply(state, Action::Player(PlayerAction::DrawEncounterCard));
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "multi-investigator spawn suspends, got {:?}",
        r1.outcome,
    );
    assert!(
        r1.state.spawn_engage_pending.is_some(),
        "spawn engagement tie should be pending for the lead's pick",
    );
    let spawned = r1.state.enemies.values().next().expect("enemy placed");
    assert_eq!(spawned.engaged_with, None, "engagement deferred until pick");

    // 2) Lead picks investigator 2; engagement resolves, draw chain ends.
    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickInvestigator(InvestigatorId(2)),
        }),
    );
    assert_eq!(r2.outcome, EngineOutcome::Done);
    assert!(r2.state.spawn_engage_pending.is_none());
    let enemy = r2.state.enemies.values().next().expect("enemy in play");
    assert_eq!(enemy.engaged_with, Some(InvestigatorId(2)));
}

#[test]
fn hunter_movement_pick_location_replays_identically() {
    fn diamond_state() -> game_core::state::GameState {
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_enemy(1, "Hunter");
        hunter.hunter = true;
        hunter.current_location = Some(LocationId(1));
        TestGame::new()
            .with_phase(Phase::Investigation)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            .build()
    }

    let actions = [
        Action::Player(PlayerAction::EndTurn),
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickLocation(LocationId(3)),
        }),
    ];

    let mut s1 = diamond_state();
    for a in &actions {
        s1 = apply(s1, a.clone()).state;
    }
    let mut s2 = diamond_state();
    for a in &actions {
        s2 = apply(s2, a.clone()).state;
    }
    // Replay determinism is a whole-state property: the engine guarantees
    // that replaying an identical action log reproduces state bit-for-bit.
    // GameState isn't PartialEq, and its maps are BTreeMaps (stable key
    // order), so comparing the full serialized form is the right, strongest
    // check here — stricter than the field-wise comparisons used elsewhere.
    assert_eq!(
        serde_json::to_string(&s1).unwrap(),
        serde_json::to_string(&s2).unwrap(),
        "replaying the same action log must reproduce identical state",
    );
    assert_eq!(
        s1.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
}
