//! "They're Getting Out!" (agenda 01107) forced abilities through the real
//! card registry: the enemy-phase-end move + round-end doom fire via the
//! forced-trigger path and the `Effect::Native` bridge end-to-end. Also
//! covers round-end ordering — the act's "when the round ends" window
//! resolves before this agenda's "at the end of the round" doom (RR `when`
//! before `at`).

use game_core::action::InputResponse;
use game_core::engine::TimingEvent;
use game_core::state::{
    Act, Agenda, CardCode, Continuation, Enemy, EnemyId, GameState, InvestigatorId, Location,
    LocationId, Phase, TimingMode,
};
use game_core::test_support::{
    fire_forced_on_phase_end, fire_forced_on_round_end, resume_round_end_window,
    run_upkeep_round_end, test_enemy, test_investigator, GameStateBuilder,
};
use game_core::{EngineOutcome, Event};

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

fn ghoul(id: u32, at: LocationId) -> Enemy {
    let mut e = test_enemy(id, "Ghoul");
    e.traits = vec!["Humanoid".into(), "Monster".into(), "Ghoul".into()];
    e.current_location = Some(at);
    e
}

fn board_with_agenda() -> GameState {
    let loc = |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_investigator(1))
        .with_turn_order([InvestigatorId(1)])
        .with_location(loc(2, "01112", "Hallway"))
        .with_location(loc(5, "01115", "Parlor"))
        .with_phase(Phase::Enemy)
        .build();
    state.connect(LocationId(2), LocationId(5));
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01107"),
        doom_threshold: 10,
        resolution: None,
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn enemy_phase_end_moves_ghoul_toward_parlor() {
    let mut state = board_with_agenda();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
    let mut events = Vec::new();
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(5)),
        "Ghoul stepped Hallway -> Parlor"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::EnemyMoved { to, .. } if *to == LocationId(5))));
}

#[test]
fn round_end_places_doom_per_ghoul_in_hallway_or_parlor() {
    let mut state = board_with_agenda();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2)));
    state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5)));
    let mut events = Vec::new();
    let outcome = fire_forced_on_round_end(&mut state, &mut events);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.agenda_doom, 2, "1 doom per Ghoul in Hallway/Parlor");
}

#[test]
fn round_end_act_when_window_opens_before_agenda_at_doom() {
    // Act 01109 ("The Barrier") carries the "when the round ends" clue-spend
    // window; agenda 01107 carries the "at the end of the round" doom. Per the
    // RR "At" entry, `when` resolves before `at`, so the act window must open
    // BEFORE any doom is placed.
    let mut state = board_with_agenda();
    state.phase = Phase::Upkeep;
    // UpkeepPhase anchor (slice 1a): the round-end teardown pops it.
    state
        .continuations
        .push(game_core::state::Continuation::UpkeepPhase {
            resume: game_core::state::UpkeepResume::Begins,
        });

    // Affordable act window: investigator in the Hallway (01112) with >= 3 clues.
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 3,
        resolution: None,
    }];
    state.act_index = 0;
    {
        let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
        inv.current_location = Some(LocationId(2)); // Hallway
        inv.clues = 3;
    }
    // Two Ghouls in Hallway/Parlor -> agenda 01107 would place 2 doom.
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2)));
    state.enemies.insert(EnemyId(2), ghoul(2, LocationId(5)));

    let mut events = Vec::new();
    let out = run_upkeep_round_end(&mut state, &mut events);

    // The act's `when the round ends` window opens first...
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    assert!(matches!(
        state.continuations.last(),
        Some(Continuation::TimingPointWindow {
            event: TimingEvent::RoundEnded,
            mode: TimingMode::Reaction,
            ..
        })
    ));
    // ...and the agenda's `at the end of the round` doom is NOT placed yet.
    assert_eq!(
        state.agenda_doom, 0,
        "`when` resolves before `at`: doom must wait for the act window"
    );

    // Declining the `when` window then runs the `at` doom (2: one per Ghoul in
    // Hallway/Parlor). The exact total isn't pinned because the Skip cascades
    // through step_phase into the next Mythos phase, whose step 1.2 places a
    // further doom on the agenda — so `>= 2` (the `at` doom landed) is the
    // assertion that isolates this test's concern from the downstream cascade.
    let _ = resume_round_end_window(&mut state, &mut events, &InputResponse::Skip);
    assert!(
        state.agenda_doom >= 2,
        "the `at` doom lands after the act window resolves"
    );
}
