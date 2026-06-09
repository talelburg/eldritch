//! Phase-4 closing demo: two end-to-end walks over the synthetic
//! fixture, each cycling Mythos -> Investigation -> Enemy -> Upkeep with
//! real actions and ending in a resolution, each verified deterministic
//! by a serialize round-trip mid-scenario.
//!
//! Lives in `crates/scenarios/tests/` (its own process) so it can
//! `install` the process-global registries without colliding with
//! `game-core`'s unit tests, and so it can reach the real
//! `scenarios::REGISTRY` + synthetic card corpus.

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::{CardCode, GameState, InvestigatorId, LocationId, Phase};
use game_core::{assert_event, Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::{
    SYNTH_ENEMY_CODE, SYNTH_TREACHERY_CODE, TEST_REGISTRY,
};
use scenarios::REGISTRY;

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

/// Apply one action, asserting it is not `Rejected` so a mis-ordered
/// step fails loudly here (naming the offending action) rather than
/// surfacing later as a confusing "event not found". Returns the new
/// state and emitted events.
fn apply_checked(state: GameState, action: &Action) -> (GameState, Vec<Event>) {
    let r = apply(state, action.clone());
    assert!(
        !matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "action {action:?} was rejected: {:?}",
        r.outcome,
    );
    (r.state, r.events)
}

/// Apply `actions` in order from `initial`, concatenating all emitted
/// events. The log includes explicit `ResolveInput { CommitCards }` steps
/// for each skill-test commit window. Each action is run through
/// [`apply_checked`], so a `Rejected` step fails loudly.
fn drive(mut state: GameState, actions: &[Action]) -> (GameState, Vec<Event>) {
    let mut events = Vec::new();
    for a in actions {
        let (next, ev) = apply_checked(state, a);
        state = next;
        events.extend(ev);
    }
    (state, events)
}

/// Replay-determinism with a serialize round-trip: drive `log` from a
/// fresh `make_initial()` to the midpoint, serialize -> deserialize,
/// then continue. Returns the round-tripped final state. The serde step
/// at the midpoint is the *only* delta from a straight `drive` of the
/// same log, so comparing this result to the un-round-tripped final
/// state isolates serde round-trip fidelity — the property Phase 5's
/// persistence depends on — on top of the seeded-`rng` replay path both
/// runs share.
///
/// The midpoint split lands mid-walk — for the Won walk inside an active
/// skill-test commit window (`in_flight_skill_test` is `Some`), for the
/// Lost walk on a phase boundary — so this exercises serde of in-flight
/// engine state, not just a clean resting point.
fn replay_with_roundtrip(make_initial: impl Fn() -> GameState, log: &[Action]) -> GameState {
    let split = log.len() / 2;
    let mut state = make_initial();
    for a in &log[..split] {
        state = apply(state, a.clone()).state;
    }
    let json = serde_json::to_string(&state).expect("serialize mid-scenario state");
    let mut state: GameState = serde_json::from_str(&json).expect("deserialize mid-scenario state");
    for a in &log[split..] {
        state = apply(state, a.clone()).state;
    }
    state
}

#[test]
fn won_walk_full_cycle_replays_identically() {
    install_registry();
    let inv = InvestigatorId(1);

    // Raw setup() with no local seeding — exactly the state the browser
    // plays. setup() itself must place the investigator at the demo
    // location, stock it with clues, and seed a non-empty chaos bag; this
    // walk is the regression guard for that (P6.8 demo playability).
    let make_initial = scenarios::test_fixtures::synthetic::setup;

    // Round 1 (Mythos skipped): Investigate x3 (each followed by a
    // CommitCards round-trip for the skill-test commit window) ->
    // EndTurn cascades through Enemy/Upkeep -> pauses at round-2 Mythos
    // 1.4 -> DrawEncounterCard finishes Mythos -> round 2 Investigate
    // (4th clue, +commit) -> AdvanceAct x2 (act 0 -> 1 -> Won).
    let commit_nothing = Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::CommitCards { indices: vec![] },
    });
    let log = vec![
        Action::Player(PlayerAction::StartScenario),
        Action::Player(PlayerAction::Mulligan {
            investigator: inv,
            indices_to_redraw: vec![],
        }),
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        commit_nothing.clone(), // commit window for investigate 1
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        commit_nothing.clone(), // commit window for investigate 2
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        commit_nothing.clone(), // commit window for investigate 3
        Action::Player(PlayerAction::EndTurn),
        Action::Player(PlayerAction::DrawEncounterCard),
        Action::Player(PlayerAction::Investigate { investigator: inv }),
        commit_nothing.clone(), // commit window for investigate 4
        Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
        Action::Player(PlayerAction::AdvanceAct { investigator: inv }),
    ];

    let (final_state, events) = drive(make_initial(), &log);

    // Cycled all four phases across the two rounds.
    assert_event!(events, Event::PhaseEnded { phase } if *phase == Phase::Investigation);
    assert_event!(events, Event::PhaseStarted { phase } if *phase == Phase::Enemy);
    assert_event!(events, Event::PhaseStarted { phase } if *phase == Phase::Upkeep);
    assert_event!(events, Event::PhaseStarted { phase } if *phase == Phase::Mythos);
    // Investigation discovered clues; the act advanced; the scenario was won.
    assert_event!(events, Event::ActAdvanced { from } if *from == 0);
    assert_event!(
        events,
        Event::ScenarioResolved { resolution: Resolution::Won { id } } if id == "demo"
    );
    assert!(matches!(
        final_state.resolution,
        Some(Resolution::Won { .. })
    ));

    let replayed = replay_with_roundtrip(make_initial, &log);
    assert_eq!(
        final_state, replayed,
        "Won walk must replay identically across a serialize round-trip",
    );
}

#[test]
fn lost_walk_spawn_attack_doom_replays_identically() {
    install_registry();
    let inv = InvestigatorId(1);

    // setup() + place the investigator at the demo location so the
    // spawn-bearing enemy can engage on arrival, then seed the encounter
    // deck with the enemy on top so the first Mythos draw spawns it.
    let make_initial = || {
        let mut s = scenarios::test_fixtures::synthetic::setup();
        s.investigators.get_mut(&inv).unwrap().current_location = Some(LocationId(10));
        scenarios::test_fixtures::synthetic::with_encounter_deck(
            &mut s,
            vec![
                CardCode(SYNTH_ENEMY_CODE.into()),
                CardCode(SYNTH_TREACHERY_CODE.into()),
            ],
        );
        s
    };

    // Setup + close mulligan, then drive an EndTurn cascade, drawing only
    // when a Mythos draw is pending and breaking on resolution. Record the
    // realized action log so the round-trip replays exactly what ran.
    let mut log = vec![
        Action::Player(PlayerAction::StartScenario),
        Action::Player(PlayerAction::Mulligan {
            investigator: inv,
            indices_to_redraw: vec![],
        }),
    ];
    let (mut state, mut events) = drive(make_initial(), &log);

    // 12 iterations is ~2x headroom: doom +1 per Mythos and the two
    // agendas sum to a threshold of 4, so the loss latches by ~round 5.
    for _ in 0..12 {
        let act = Action::Player(PlayerAction::EndTurn);
        let (next, ev) = apply_checked(state, &act);
        log.push(act);
        state = next;
        events.extend(ev);
        if state.resolution.is_some() {
            break;
        }
        if state.mythos_draw_pending.is_some() {
            let act = Action::Player(PlayerAction::DrawEncounterCard);
            let (next, ev) = apply_checked(state, &act);
            log.push(act);
            state = next;
            events.extend(ev);
            if state.resolution.is_some() {
                break;
            }
        }
    }

    // Enemy spawned, engaged, and attacked (proven by EnemyExhausted —
    // the synthetic enemy deals 0 damage, so no DamageTaken fires).
    assert_event!(events, Event::EnemySpawned { code, .. } if code.0 == SYNTH_ENEMY_CODE);
    assert_event!(events, Event::EnemyEngaged { investigator, .. } if *investigator == inv);
    assert_event!(events, Event::EnemyExhausted { .. });
    // Doom advanced the agenda and then latched the loss.
    assert_event!(events, Event::AgendaAdvanced { from } if *from == 0);
    assert_event!(
        events,
        Event::ScenarioResolved {
            resolution: Resolution::Lost { .. }
        }
    );
    assert!(matches!(state.resolution, Some(Resolution::Lost { .. })));

    let replayed = replay_with_roundtrip(make_initial, &log);
    assert_eq!(
        state, replayed,
        "Lost walk must replay identically across a serialize round-trip",
    );
}
