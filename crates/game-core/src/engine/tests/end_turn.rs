use super::*;

/// Whether `state`'s menu offers an `EndTurn`.
fn offers_end_turn(state: &GameState) -> bool {
    legal_actions(state)
        .iter()
        .any(|a| matches!(a, TurnAction::EndTurn))
}

/// `roster` seated through real setup, settled at its mulligan.
fn seated_at_mulligan(roster: &[RosterEntry]) -> TestSession {
    TestSession::new(seat_and_open(GameStateBuilder::new().build(), roster).state)
}

#[test]
fn end_turn_drains_actions_and_emits_turn_ended() {
    let inv2 = InvestigatorId(2);
    let session = GameStateBuilder::new()
        .with_seat(seat(1))
        .with_seat(seat(2))
        .with_turn_order([INV, inv2])
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn);

    // A mid-round EndTurn rotates to the next investigator's turn.
    session.assert_at_turn_menu(inv2);
    assert_event!(
        session.events(),
        Event::ActionsRemainingChanged { investigator, new_count: 0 } if *investigator == INV
    );
    assert_event!(
        session.events(),
        Event::TurnEnded { investigator } if *investigator == INV
    );
    assert_eq!(session.state().investigators[&INV].actions_remaining, 0);
}

#[test]
fn end_turn_with_no_active_investigator_is_rejected() {
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .build();

    // No active investigator → EndTurn is not a legal open-turn action.
    assert!(!offers_end_turn(&state));
}

#[test]
fn end_turn_outside_investigation_phase_is_rejected() {
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_seat(seat(1))
        .with_active_investigator(INV)
        .build();

    // Mythos phase → EndTurn is not a legal open-turn action.
    assert!(!offers_end_turn(&state));
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
    let session = seated_at_mulligan(&roster);
    let state = session.state();
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

    // Mulligan past the loop for both investigators (keep each hand). The
    // engine requires every investigator to mulligan before the turn opens.
    let session = session.choose_from_hand(&[]).choose_from_hand(&[]);
    // After the last mulligan, the Investigation phase begins and the
    // lead investigator becomes active.
    session.assert_at_turn_menu(inv1);

    // First EndTurn (inv1): rotates to inv2 within Investigation.
    // No phase transitions yet.
    let before = session.events().len();
    let session = session.take(&TurnAction::EndTurn);
    session.assert_at_turn_menu(inv2);
    let state = session.state();
    assert_eq!(state.round, 1);
    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(state.investigators[&inv1].actions_remaining, 0);
    assert_eq!(state.investigators[&inv2].actions_remaining, 3);
    let events = &session.events()[before..];
    assert_event!(
        events,
        Event::TurnEnded { investigator } if *investigator == inv1
    );
    // rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2)
    assert_no_event!(
        events,
        Event::ActionsRemainingChanged { investigator, .. } if *investigator == inv2
    );
    // No phase transition on a mid-round EndTurn.
    assert_no_event!(events, Event::PhaseEnded { .. });
    assert_no_event!(events, Event::PhaseStarted { .. });
    assert_no_event!(events, Event::ScenarioStarted);

    // Second EndTurn (inv2, last in turn_order): auto-advances through
    // Investigation → Enemy → Upkeep → Mythos and then PAUSES at the
    // step-1.4 encounter-draw prompt for inv1. The phase chain does NOT
    // continue to Investigation — that waits for the encounter draws.
    let before = session.events().len();
    let session = session.take(&TurnAction::EndTurn);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::EncounterDeck),
        "round-ending EndTurn pauses at the Mythos draw prompt"
    );
    let state = session.state();
    assert_eq!(state.round, 2, "round bumps on Mythos entry");
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.current_encounter_drawer(),
        Some(inv1),
        "lead investigator (inv1) draws first"
    );
    let events = &session.events()[before..];

    // Exactly 3 PhaseEnded events fire (Investigation, Enemy, Upkeep).
    // PhaseEnded(Mythos) does NOT fire here — mythos_phase_end owns it
    // and runs only after DrawEncounterCard completes the chain.
    assert_event_count!(events, 3, Event::PhaseEnded { .. });
    for phase in [Phase::Investigation, Phase::Enemy, Phase::Upkeep] {
        assert_event!(events, Event::PhaseEnded { phase: p } if *p == phase);
    }
    assert_no_event!(
        events,
        Event::PhaseEnded { phase: p } if *p == Phase::Mythos
    );

    // Exactly 3 PhaseStarted events fire (Enemy, Upkeep, Mythos).
    // PhaseStarted(Investigation) does NOT fire here — investigation_phase
    // runs only after mythos_phase_end, which runs after DrawEncounterCard.
    assert_event_count!(events, 3, Event::PhaseStarted { .. });
    for phase in [Phase::Enemy, Phase::Upkeep, Phase::Mythos] {
        assert_event!(events, Event::PhaseStarted { phase: p } if *p == phase);
    }
    assert_no_event!(
        events,
        Event::PhaseStarted { phase: p } if *p == Phase::Investigation
    );

    // EndTurn must never re-emit ScenarioStarted.
    assert_no_event!(events, Event::ScenarioStarted);
}

#[test]
fn last_end_turn_advances_to_mythos_and_pauses_for_draw_solo() {
    // Degenerate edge: with only one investigator in turn_order,
    // their single EndTurn is also the *last* EndTurn of the round.
    // It must auto-advance Investigation → Enemy → Upkeep → Mythos,
    // bump the round, prompt the encounter draw for id, and then
    // PAUSE. It does NOT complete the full cycle — that requires the
    // subsequent encounter draw (needs registry, covered by
    // crates/cards/tests/mythos_phase.rs).
    let roster = vec![RosterEntry {
        investigator: CardCode::new(test_support::TEST_INV),
        deck: vec![],
    }];
    // seat_and_open: round 0 → 1, mulligan window opens.
    // active_investigator is None until mulligan completion.
    let session = seated_at_mulligan(&roster);
    assert_eq!(session.state().round, 1);
    assert_eq!(
        session.state().active_investigator,
        None,
        "active investigator not set until mulligan window closes"
    );

    // Mulligan past the setup loop. After completion, lead becomes active.
    let session = session.choose_from_hand(&[]);
    session.assert_at_turn_menu(INV);

    let before = session.events().len();
    let session = session.take(&TurnAction::EndTurn);
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::EncounterDeck),
        "round-ending EndTurn pauses at the Mythos draw prompt"
    );
    let state = session.state();
    assert_eq!(state.round, 2, "round bumps on Mythos entry");
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.current_encounter_drawer(),
        Some(INV),
        "sole investigator is the pending drawer"
    );

    // The partial event chain: 3 PhaseEnded (Investigation, Enemy, Upkeep)
    // and 3 PhaseStarted (Enemy, Upkeep, Mythos). The Mythos-side
    // pair (PhaseEnded(Mythos) + PhaseStarted(Investigation)) fires
    // later via mythos_phase_end after DrawEncounterCard resolves.
    let events = &session.events()[before..];
    assert_event_count!(events, 3, Event::PhaseEnded { .. });
    assert_event_count!(events, 3, Event::PhaseStarted { .. });
    for phase in [Phase::Investigation, Phase::Enemy, Phase::Upkeep] {
        assert_event!(events, Event::PhaseEnded { phase: p } if *p == phase);
    }
    assert_no_event!(
        events,
        Event::PhaseEnded { phase: p } if *p == Phase::Mythos
    );
    for phase in [Phase::Enemy, Phase::Upkeep, Phase::Mythos] {
        assert_event!(events, Event::PhaseStarted { phase: p } if *p == phase);
    }
    assert_no_event!(
        events,
        Event::PhaseStarted { phase: p } if *p == Phase::Investigation
    );
}
