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

use game_core::engine::apply;
use game_core::event::Event;
use game_core::scenario::Resolution;
use game_core::state::{
    ChaosBag, ChaosToken, GameState, InvestigatorId, LocationId, Phase, TokenModifiers,
};
use game_core::{assert_event, Action, InputResponse, PlayerAction};
use scenarios::test_fixtures::synth_cards::TEST_REGISTRY;
use scenarios::REGISTRY;

static INSTALL: Once = Once::new();

fn install_registry() {
    INSTALL.call_once(|| {
        let _ = game_core::scenario_registry::install(REGISTRY);
        let _ = game_core::card_registry::install(TEST_REGISTRY);
    });
}

/// Apply `actions` in order from `initial`, concatenating all emitted
/// events. The log includes explicit `ResolveInput { CommitCards }` steps
/// for each skill-test commit window, so no action is silently skipped.
fn drive(mut state: GameState, actions: &[Action]) -> (GameState, Vec<Event>) {
    let mut events = Vec::new();
    for a in actions {
        let r = apply(state, a.clone());
        events.extend(r.events);
        state = r.state;
    }
    (state, events)
}

/// Replay-determinism with a serialize round-trip: drive `log` from a
/// fresh `make_initial()` to the midpoint, serialize -> deserialize,
/// then continue. Returns the round-tripped final state. Proves both
/// replay determinism (seeded `state.rng` reproduces draws) and serde
/// round-trip fidelity (the property Phase 5's persistence depends on).
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

    // setup() + deterministic local seeding: 4 clues to discover and a
    // +0 chaos bag so Investigate succeeds against shroud 2 (intellect 3).
    let make_initial = || {
        let mut s = scenarios::test_fixtures::synthetic::setup();
        s.locations.get_mut(&LocationId(10)).unwrap().clues = 4;
        s.chaos_bag = ChaosBag::new([ChaosToken::Numeric(0)]);
        s.token_modifiers = TokenModifiers::default();
        // Place the investigator at the demo location so Investigate
        // can resolve (dispatch rejects if current_location is None).
        s.investigators.get_mut(&inv).unwrap().current_location = Some(LocationId(10));
        s
    };

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
