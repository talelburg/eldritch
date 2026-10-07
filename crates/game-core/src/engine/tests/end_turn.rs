use super::*;

#[test]
fn end_turn_drains_actions_and_emits_turn_ended() {
    let id = InvestigatorId(1);
    let mut roland = test_support::test_investigator(1);
    roland.actions_remaining = 3;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(roland)
        .with_investigator(test_support::test_investigator(2))
        .with_turn_order([id, InvestigatorId(2)])
        .with_active_investigator(id)
        .with_phase_anchor(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins,
        })
        .with_investigator_turn(id)
        .build();

    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);

    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 0 } if *investigator == id
    );
    assert_event!(
        result.events,
        Event::TurnEnded { investigator } if *investigator == id
    );
    assert_eq!(result.state.investigators[&id].actions_remaining, 0);
}

#[test]
fn end_turn_with_no_active_investigator_is_rejected() {
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .build();

    // No active investigator → EndTurn is not a legal open-turn action.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::EndTurn)));
}

#[test]
fn end_turn_outside_investigation_phase_is_rejected() {
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(id)
        .build();

    // Mythos phase → EndTurn is not a legal open-turn action.
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::EndTurn)));
}

// After T09, end_turn pauses at Mythos when mythos_draw_pending is
// Some(_). The player-driven DrawEncounterCard action (T12) is what
// completes the Mythos phase; it requires a card registry which
// game-core unit tests cannot install (process-global OnceLock;
// installing in one test would contaminate others). The full
// round-cycle coverage — including DrawEncounterCard completing the
// phase and transitioning to Investigation — lives in the T14
// integration tests at crates/cards/tests/mythos_phase.rs.
//
// These two tests verify the pause-at-Mythos shape: the last EndTurn
// in a round must land in Mythos with mythos_draw_pending populated,
// with the correct partial event chain (Investigation/Enemy/Upkeep
// boundaries + PhaseStarted(Mythos)). PhaseEnded(Mythos) and
// PhaseStarted(Investigation) do NOT fire here — they fire later via
// the DrawEncounterCard → mythos_phase_end continuation.
#[test]
fn last_end_turn_advances_to_mythos_and_pauses_for_draw_two_investigators() {
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    test_support::install_test_registry();
    let state = GameStateBuilder::new().build();
    let roster = vec![
        RosterEntry {
            investigator: CardCode::new(test_support::TEST_INV),
            deck: vec![],
        };
        2
    ];

    // seat_and_open: round 0 → 1, phase Investigation (mulligan window
    // open). The Investigation phase does NOT begin until the last
    // investigator mulligans — active_investigator is None here.
    let result = seat_and_open(state, &roster);
    let state = result.state;
    assert_eq!(state.round, 1);
    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(
        state.current_mulligan(),
        Some(inv1),
        "mulligan loop must prompt the first investigator after seat_and_open"
    );
    assert_eq!(
        state.active_investigator, None,
        "active investigator not yet set — Investigation phase begins after mulligan"
    );
    assert_eq!(state.investigators[&inv1].actions_remaining, 3);

    // Mulligan past the loop for both investigators (empty redraws =
    // "keep my hand"), each via `ResolveInput(PickMultiple)`. The engine
    // requires every investigator to mulligan before non-`ResolveInput`
    // actions are accepted.
    let empty_mulligan = || {
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        })
    };
    let state = apply(state, empty_mulligan()).state;
    let state = apply(state, empty_mulligan()).state;
    // After the last mulligan, the Investigation phase begins and the
    // lead investigator becomes active.
    assert_eq!(
        state.active_investigator,
        Some(inv1),
        "lead investigator becomes active after mulligan window closes"
    );

    // First EndTurn (inv1): rotates to inv2 within Investigation.
    // No phase transitions yet.
    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);
    let state = result.state;
    assert_eq!(state.round, 1);
    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(state.active_investigator, Some(inv2));
    assert_eq!(state.investigators[&inv1].actions_remaining, 0);
    assert_eq!(state.investigators[&inv2].actions_remaining, 3);
    assert_event!(
        result.events,
        Event::TurnEnded { investigator } if *investigator == inv1
    );
    // rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2)
    assert_no_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, .. } if *investigator == inv2
    );
    // No phase transition on a mid-round EndTurn.
    assert_no_event!(result.events, Event::PhaseEnded { .. });
    assert_no_event!(result.events, Event::PhaseStarted { .. });
    assert_no_event!(result.events, Event::ScenarioStarted);

    // Second EndTurn (inv2, last in turn_order): auto-advances through
    // Investigation → Enemy → Upkeep → Mythos and then PAUSES at the
    // step-1.4 encounter-draw prompt for inv1. The phase chain does NOT
    // continue to Investigation — that waits for the ResolveInput(Confirm)s.
    let result = test_support::take_turn_action(state, &TurnAction::EndTurn);
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "round-ending EndTurn pauses at the Mythos draw prompt, got {:?}",
        result.outcome
    );
    let state = result.state;
    assert_eq!(state.round, 2, "round bumps on Mythos entry");
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.current_encounter_drawer(),
        Some(inv1),
        "lead investigator (inv1) draws first"
    );

    // Exactly 3 PhaseEnded events fire (Investigation, Enemy, Upkeep).
    // PhaseEnded(Mythos) does NOT fire here — mythos_phase_end owns it
    // and runs only after DrawEncounterCard completes the chain.
    assert_event_count!(result.events, 3, Event::PhaseEnded { .. });
    for phase in [Phase::Investigation, Phase::Enemy, Phase::Upkeep] {
        assert_event!(result.events, Event::PhaseEnded { phase: p } if *p == phase);
    }
    assert_no_event!(
        result.events,
        Event::PhaseEnded { phase: p } if *p == Phase::Mythos
    );

    // Exactly 3 PhaseStarted events fire (Enemy, Upkeep, Mythos).
    // PhaseStarted(Investigation) does NOT fire here — investigation_phase
    // runs only after mythos_phase_end, which runs after DrawEncounterCard.
    assert_event_count!(result.events, 3, Event::PhaseStarted { .. });
    for phase in [Phase::Enemy, Phase::Upkeep, Phase::Mythos] {
        assert_event!(result.events, Event::PhaseStarted { phase: p } if *p == phase);
    }
    assert_no_event!(
        result.events,
        Event::PhaseStarted { phase: p } if *p == Phase::Investigation
    );

    // EndTurn must never re-emit ScenarioStarted.
    assert_no_event!(result.events, Event::ScenarioStarted);
}

#[test]
fn last_end_turn_advances_to_mythos_and_pauses_for_draw_solo() {
    // Degenerate edge: with only one investigator in turn_order,
    // their single EndTurn is also the *last* EndTurn of the round.
    // It must auto-advance Investigation → Enemy → Upkeep → Mythos,
    // bump the round, prompt the encounter draw for id, and then
    // PAUSE. It does NOT complete the full cycle — that requires the
    // subsequent ResolveInput(Confirm) (needs registry, covered by
    // crates/cards/tests/mythos_phase.rs).
    test_support::install_test_registry();
    let state = GameStateBuilder::new().build();
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: vec![],
    }];
    // seat_and_open: round 0 → 1, mulligan window opens.
    // active_investigator is None until mulligan completion.
    let after_start = seat_and_open(state, &roster).state;
    let id = InvestigatorId(1);
    assert_eq!(after_start.round, 1);
    assert_eq!(
        after_start.active_investigator, None,
        "active investigator not set until mulligan window closes"
    );

    // Mulligan past the setup loop. After completion, lead becomes active.
    let after_mulligan = apply(
        after_start,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    )
    .state;

    let result = test_support::take_turn_action(after_mulligan, &TurnAction::EndTurn);
    assert!(
        matches!(result.outcome, EngineOutcome::AwaitingInput { .. }),
        "round-ending EndTurn pauses at the Mythos draw prompt, got {:?}",
        result.outcome
    );
    assert_eq!(result.state.round, 2, "round bumps on Mythos entry");
    assert_eq!(result.state.phase, Phase::Mythos);
    assert_eq!(
        result.state.current_encounter_drawer(),
        Some(id),
        "sole investigator is the pending drawer"
    );

    // The partial event chain: 3 PhaseEnded (Investigation, Enemy, Upkeep)
    // and 3 PhaseStarted (Enemy, Upkeep, Mythos). The Mythos-side
    // pair (PhaseEnded(Mythos) + PhaseStarted(Investigation)) fires
    // later via mythos_phase_end after DrawEncounterCard resolves.
    assert_event_count!(result.events, 3, Event::PhaseEnded { .. });
    assert_event_count!(result.events, 3, Event::PhaseStarted { .. });
    for phase in [Phase::Investigation, Phase::Enemy, Phase::Upkeep] {
        assert_event!(result.events, Event::PhaseEnded { phase: p } if *p == phase);
    }
    assert_no_event!(
        result.events,
        Event::PhaseEnded { phase: p } if *p == Phase::Mythos
    );
    for phase in [Phase::Enemy, Phase::Upkeep, Phase::Mythos] {
        assert_event!(result.events, Event::PhaseStarted { phase: p } if *p == phase);
    }
    assert_no_event!(
        result.events,
        Event::PhaseStarted { phase: p } if *p == Phase::Investigation
    );
}
