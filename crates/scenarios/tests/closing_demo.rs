//! Phase-4 closing demo: two end-to-end walks over the synthetic
//! fixture, each cycling Mythos -> Investigation -> Enemy -> Upkeep with
//! real actions and ending in a resolution, each verified deterministic
//! by a serialize round-trip mid-scenario.
//!
//! Lives in `crates/scenarios/tests/` (its own process) so it can
//! `install` the process-global registries without colliding with
//! `game-core`'s unit tests, and so it can reach the real
//! `scenarios::REGISTRY` + synthetic card corpus.

use game_core::action::RosterEntry;
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::seat_and_open;
use game_core::state::{CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::TEST_INV;
use game_core::{assert_event, Action, InputResponse, PlayerAction, TurnAction};
use scenarios::test_fixtures::synth_cards::{
    SYNTH_ENEMY_CODE, SYNTH_TREACHERY_CODE, TEST_REGISTRY,
};
use scenarios::REGISTRY;

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::scenario_registry::install(REGISTRY);
    let _ = game_core::card_registry::install(TEST_REGISTRY);
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

/// Drive a single open-turn action via the enumeration path (same as
/// `take_turn_action`) and record the resulting `ResolveInput(PickSingle)`
/// wire action in `log`, so the log can be replayed identically.
fn take_checked(
    state: GameState,
    action: &TurnAction,
    log: &mut Vec<Action>,
) -> (GameState, Vec<Event>) {
    let legal = game_core::engine::legal_actions(&state);
    let idx = legal
        .iter()
        .position(|a| a == action)
        .unwrap_or_else(|| panic!("take_checked: {action:?} is not legal; offered: {legal:?}"));
    let wire = Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(game_core::engine::OptionId(
            u32::try_from(idx).expect("idx fits u32"),
        )),
    });
    log.push(wire.clone());
    apply_checked(state, &wire)
}

/// Apply `actions` in order from `initial`, concatenating all emitted
/// events. The log includes explicit `ResolveInput { PickMultiple }` steps
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
    let inv = InvestigatorId(1);

    // make_initial folds seat_and_open in: the log starts from the post-seat
    // state so replay_with_roundtrip rebuilds from the same starting point.
    // setup() stocks the demo location with clues and seeds a non-empty chaos
    // bag; this walk is the regression guard for that (P6.8 demo playability).
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: vec![],
    }];
    let make_initial =
        || seat_and_open(scenarios::test_fixtures::synthetic::setup(), &roster).state;

    // Round 1 (Mythos skipped): Investigate x3 (each followed by a
    // PickMultiple round-trip for the skill-test commit window) ->
    // EndTurn cascades through Enemy/Upkeep -> pauses at round-2 Mythos
    // 1.4 -> DrawEncounterCard finishes Mythos -> round 2 Investigate
    // (4th clue, +commit) -> AdvanceAct x2 (act 0 -> 1 -> Won).
    //
    // The log is built dynamically: open-turn actions are driven via
    // take_checked, which enumerates legal actions, picks the right option
    // id, and records the resulting ResolveInput(PickSingle) wire action so
    // the log can be replayed identically (replay_with_roundtrip).
    let commit_nothing = Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple { selected: vec![] },
    });
    let draw_encounter = Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::Confirm,
    });

    let mut log: Vec<Action> = Vec::new();
    let mut state = make_initial();
    let mut events: Vec<Event> = Vec::new();

    macro_rules! push_apply {
        ($action:expr) => {{
            let a = $action;
            log.push(a.clone());
            let (s, ev) = apply_checked(state, &a);
            state = s;
            events.extend(ev);
        }};
    }

    // Mulligan (seat_and_open is in make_initial, not the log).
    push_apply!(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple { selected: vec![] },
    }));

    // Investigate x3 (each with a commit window).
    for _ in 0..3 {
        let (s, ev) = take_checked(
            state,
            &TurnAction::Investigate { investigator: inv },
            &mut log,
        );
        state = s;
        events.extend(ev);
        push_apply!(commit_nothing.clone());
    }

    // EndTurn → cascades through phases, pauses at Mythos 1.4.
    let (s, ev) = take_checked(state, &TurnAction::EndTurn, &mut log);
    state = s;
    events.extend(ev);

    // DrawEncounterCard (Confirm).
    push_apply!(draw_encounter.clone());

    // Round 2: Investigate (4th clue) + commit window.
    let (s, ev) = take_checked(
        state,
        &TurnAction::Investigate { investigator: inv },
        &mut log,
    );
    state = s;
    events.extend(ev);
    push_apply!(commit_nothing.clone());

    // AdvanceAct x2 (act 0 → 1 → Won).
    let (s, ev) = take_checked(
        state,
        &TurnAction::AdvanceAct { investigator: inv },
        &mut log,
    );
    state = s;
    events.extend(ev);
    let (s, ev) = take_checked(
        state,
        &TurnAction::AdvanceAct { investigator: inv },
        &mut log,
    );
    state = s;
    events.extend(ev);

    let final_state = state;

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
    let inv = InvestigatorId(1);

    // make_initial folds seat_and_open in (investigator is placed at
    // starting_location = LocationId(10) by seat_and_open), then seeds the
    // encounter deck with the enemy on top so the first Mythos draw spawns it.
    let roster = vec![RosterEntry {
        investigator: CardCode::new(TEST_INV),
        deck: vec![],
    }];
    let make_initial = || {
        let mut s = seat_and_open(scenarios::test_fixtures::synthetic::setup(), &roster).state;
        scenarios::test_fixtures::synthetic::with_encounter_deck(
            &mut s,
            vec![
                CardCode(SYNTH_ENEMY_CODE.into()),
                CardCode(SYNTH_TREACHERY_CODE.into()),
            ],
        );
        s
    };

    // Close mulligan (seat_and_open is in make_initial, not the log), then
    // drive an EndTurn cascade, drawing only when a Mythos draw is pending
    // and breaking on resolution. Record the realized action log so the
    // round-trip replays exactly what ran.
    let mut log = vec![Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple { selected: vec![] },
    })];
    let (mut state, mut events) = drive(make_initial(), &log);

    // 12 iterations is ~2x headroom: doom +1 per Mythos and the two
    // agendas sum to a threshold of 4, so the loss latches by ~round 5.
    for _ in 0..12 {
        let (next, ev) = take_checked(state, &TurnAction::EndTurn, &mut log);
        state = next;
        events.extend(ev);
        if state.resolution.is_some() {
            break;
        }
        if state.current_encounter_drawer().is_some() {
            let act = Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::Confirm,
            });
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
