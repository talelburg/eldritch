//! "They're Getting Out!" (agenda 01107) forced abilities through the real
//! card registry: the enemy-phase-end move + round-end doom fire via the
//! forced-trigger path and the `Effect::Native` bridge end-to-end. Also
//! covers round-end ordering — the act's "when the round ends" window
//! resolves before this agenda's "at the end of the round" doom (RR `when`
//! before `at`).

use card_dsl::dsl::EventTiming;
use game_core::action::InputResponse;
use game_core::engine::TimingEvent;
use game_core::state::{
    Act, Agenda, CardCode, Continuation, Enemy, EnemyId, GameState, InvestigatorId, Location,
    LocationId, Phase, TimingMode,
};
use game_core::test_support::{
    fire_forced_on_phase_end, fire_forced_on_round_end, resume_round_end_window,
    run_enemy_phase_end, run_upkeep_round_end, take_turn_action, test_enemy, test_investigator,
    GameStateBuilder,
};
use game_core::{EngineOutcome, Event, TurnAction};

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
    }];
    state.agenda_index = 0;
    state
}

#[test]
fn enemy_phase_end_moves_ghoul_toward_parlor() {
    let mut state = board_with_agenda();
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2))); // Hallway
    let mut events = Vec::new();
    // The `at` cell — 01107 prints *"At the end of the enemy phase"*.
    let outcome = fire_forced_on_phase_end(&mut state, &mut events, Phase::Enemy, EventTiming::At);
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

/// Regression (#569): the move must fire through the *real* step-3.4 site, not
/// only through the `fire_forced_on_phase_end` helper. `enemy_phase_end` queues
/// the agenda's forced ability as a frame and returns `Done`; before the fix it
/// read that `Done` as "nothing happened" and pushed the Upkeep anchor on top of
/// the queued frame, orphaning it at the bottom of the stack for the rest of the
/// scenario — agenda 3's Ghoul movement never happened in real play.
#[test]
fn enemy_phase_end_moves_ghoul_before_the_upkeep_transition() {
    let mut state = board_with_agenda();
    {
        // The cascade runs on into Upkeep, which reads the investigator card's
        // printed stats: give it a real code (Roland Banks) and a card to draw.
        let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
        inv.investigator_card.code = CardCode::new("01001");
        inv.current_location = Some(LocationId(2));
        inv.deck = vec![CardCode::new("01006")];
    }
    // One Ghoul in the Hallway, one step from the Parlor.
    state.enemies.insert(EnemyId(1), ghoul(1, LocationId(2)));
    // The step-3.4 site runs with the Enemy anchor on top (its
    // `AfterAllInvestigatorsAttacked` window has just closed).
    state.continuations.push(Continuation::EnemyPhase {
        resume: game_core::state::EnemyResume::AfterAllAttacked,
        attacking: None,
    });

    let mut events = Vec::new();
    let _ = run_enemy_phase_end(&mut state, &mut events);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(5)),
        "Ghoul stepped Hallway -> Parlor at the end of the enemy phase"
    );
    // Ordering is the defect: the queued ability resolves BEFORE the phase
    // transition's tail work, not after it (and not never).
    let moved = events
        .iter()
        .position(|e| matches!(e, Event::EnemyMoved { to, .. } if *to == LocationId(5)))
        .expect("the Ghoul move must fire at the enemy phase end");
    let upkeep_started = events
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::PhaseStarted {
                    phase: Phase::Upkeep
                }
            )
        })
        .expect("the Enemy -> Upkeep transition must still run");
    assert!(
        moved < upkeep_started,
        "the queued forced ability must resolve before the Upkeep phase begins; \
         events = {events:?}"
    );
}

/// #633: a Ghoul the agenda walks into the investigator's location engages on
/// arrival, and — being engaged and ready — attacks in the *next* Enemy phase.
///
/// `glossary/Enemy_Engagement.md`: *"Any time a ready unengaged enemy is at the
/// same location as an investigator, it engages that investigator, and is
/// placed in that investigator's threat area"*, listing *"It moves into the
/// same location as an investigator"*. The Ghoul here is not a Hunter, so
/// nothing else in the framework would ever pick it up: before the fix it stood
/// in the Hallway beside the investigator forever.
#[test]
fn ghoul_moved_into_the_investigator_engages_then_attacks_next_enemy_phase() {
    let mut state = board_with_agenda();
    // Add the Attic (01113) as a third room so a Ghoul can step Attic ->
    // Hallway on its way to the Parlor.
    state.locations.insert(
        LocationId(3),
        Location::new(LocationId(3), CardCode::new("01113"), "Attic", 1, 0),
    );
    state.connect(LocationId(2), LocationId(3));
    {
        let inv = state.investigators.get_mut(&InvestigatorId(1)).unwrap();
        // Hallway.
        inv.current_location = Some(LocationId(2));
        // A real investigator code so the Upkeep cascade can read printed
        // health/sanity from the installed corpus (Skids O'Toole, 01003).
        inv.investigator_card.code = CardCode::new("01003");
        inv.deck = vec![CardCode::new("01088")];
    }
    let mut walker = ghoul(1, LocationId(3)); // Attic, one step from the Hallway
    walker.attack_damage = 1;
    walker.attack_horror = 0;
    assert!(!walker.hunter, "the divergence needs a non-Hunter Ghoul");
    state.enemies.insert(EnemyId(1), walker);
    // Step 3.4 runs with the Enemy anchor on top (the
    // `AfterAllInvestigatorsAttacked` window has just closed).
    state.continuations.push(Continuation::EnemyPhase {
        resume: game_core::state::EnemyResume::AfterAllAttacked,
        attacking: None,
    });

    let mut events = Vec::new();
    let _ = run_enemy_phase_end(&mut state, &mut events);

    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2)),
        "the Ghoul stepped Attic -> Hallway"
    );
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1)),
        "and engaged the investigator it arrived on top of"
    );
    assert!(
        events.iter().any(|e| matches!(e,
            Event::EnemyEngaged { enemy, investigator }
                if *enemy == EnemyId(1) && *investigator == InvestigatorId(1))),
        "engage-on-arrival is announced: {events:?}"
    );

    // The follow-up Enemy phase: hand the (still engaged) Ghoul a round in
    // which to attack. Re-seat the carried-over state mid-Investigation and end
    // the turn — the cascade runs Investigation -> Enemy and resolves step 3.3.
    let damage_before = state.investigators[&InvestigatorId(1)].damage();
    state.phase = Phase::Investigation;
    state.active_investigator = Some(InvestigatorId(1));
    state.continuations = vec![
        Continuation::InvestigationPhase {
            resume: game_core::state::InvestigationResume::TurnBegins,
        },
        Continuation::InvestigatorTurn {
            investigator: InvestigatorId(1),
            ending: false,
        },
    ];
    // A ready enemy attacks; the Upkeep readying in between would have done
    // this anyway, but assert the precondition rather than assume it.
    assert!(!state.enemies[&EnemyId(1)].exhausted);

    let result = take_turn_action(state, &TurnAction::EndTurn);

    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
        )),
        "the now-engaged Ghoul attacks in the next Enemy phase: {:?}",
        result.events
    );
    assert!(
        result.state.investigators[&InvestigatorId(1)].damage() > damage_before,
        "the attack landed"
    );
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
