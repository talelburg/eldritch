//! #128 integration: spawn-engagement tie resolved through the real
//! registry + Mythos draw path (option A), plus hunter-movement replay
//! equality across a `PickSingle` round-trip.

use std::sync::Once;

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{apply, EngineOutcome, OptionId};
use game_core::state::{EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, TurnAction};
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
    // Drive through the real Mythos draw path: stage the EncounterDraw loop
    // frame for inv1 so the ResolveInput(Confirm) below resumes it (#348).
    state.phase = Phase::Mythos;
    // Mythos anchor (slice 1a) sits beneath the EncounterDraw loop; the
    // post-1.4 MythosAfterDraws close routes to it.
    state
        .continuations
        .push(game_core::state::Continuation::MythosPhase {
            resume: game_core::state::MythosResume::AfterDraws,
        });
    state
        .continuations
        .push(game_core::state::Continuation::EncounterDraw {
            remaining: vec![InvestigatorId(1)],
        });
    state.encounter_deck.clear();
    state
        .encounter_deck
        .push_back(game_core::state::CardCode(SYNTH_ENEMY_CODE.into()));

    // 1) Drawing the enemy suspends for the lead's PickSingle.
    let r1 = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "multi-investigator spawn suspends, got {:?}",
        r1.outcome,
    );
    assert!(
        matches!(
            r1.state.continuations.last(),
            Some(game_core::state::Continuation::SpawnEngage(_))
        ),
        "spawn engagement tie should be pending for the lead's pick",
    );
    let spawned = r1.state.enemies.values().next().expect("enemy placed");
    assert_eq!(spawned.engaged_with, None, "engagement deferred until pick");

    // 2) Lead picks investigator 2 (by its offered option id); engagement
    //    resolves, draw chain ends.
    let pick = {
        let EngineOutcome::AwaitingInput { request, .. } = &r1.outcome else {
            unreachable!("asserted AwaitingInput above");
        };
        request
            .options
            .iter()
            .find(|o| o.label == format!("{:?}", InvestigatorId(2)))
            .expect("InvestigatorId(2) among offered options")
            .id
    };
    let r2 = apply(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(pick),
        }),
    );
    assert!(matches!(r2.outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(!matches!(
        r2.state.continuations.last(),
        Some(game_core::state::Continuation::SpawnEngage(_))
    ));
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
        GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            // Mid-Investigation invariant (slice 1a): the end_turn cascade pops
            // the InvestigationPhase anchor at investigation_phase_end.
            .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
                resume: game_core::state::InvestigationResume::TurnBegins,
            })
            // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame
            // the end_turn cascade pops before advancing into the Enemy phase.
            .with_investigator_turn(InvestigatorId(1))
            .build()
    }

    // Candidates are the sorted first-steps toward D: [LocationId(2), LocationId(3)],
    // so LocationId(3) is offered option id 1.

    let mut s1 = diamond_state();
    s1 = take_turn_action(s1, &TurnAction::EndTurn).state;
    s1 = apply(
        s1,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }),
    )
    .state;
    let mut s2 = diamond_state();
    s2 = take_turn_action(s2, &TurnAction::EndTurn).state;
    s2 = apply(
        s2,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }),
    )
    .state;
    // Replay determinism is a whole-state property: replaying an
    // identical action log reproduces state bit-for-bit.
    assert_eq!(
        s1, s2,
        "replaying the same action log must reproduce identical state",
    );
    assert_eq!(
        s1.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
}
