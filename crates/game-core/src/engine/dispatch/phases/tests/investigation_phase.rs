use super::*;
use crate::assert_event;
use crate::engine::dispatch;
use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::{EngineOutcome, OptionTarget};
use crate::state::{GameStateBuilder, InvestigatorId, Phase, Status};
use crate::test_support;

#[test]
fn investigator_turn_defaults_to_not_ending() {
    // The builder-staged open-turn frame is not mid-end-turn.
    let state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .open_turn(InvestigatorId(1))
        .build();
    assert_eq!(
        state.continuations.top(),
        Some(&Continuation::InvestigatorTurn(InvestigatorTurnFrame {
            investigator: InvestigatorId(1),
            ending: false,
        })),
    );
}

#[test]
fn open_turn_leaves_investigator_turn_frame_on_top() {
    // Reach the open turn the way production does: enter the Investigation
    // phase for a single investigator (no Fast cards → windows auto-skip).
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_turn_order([InvestigatorId(1)])
        .with_phase(Phase::Investigation)
        .build();

    let mut events = Vec::new();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        // investigation_phase pushes the anchor + opens (auto-skips) both
        // windows, landing the first investigator's open turn.
        investigation_phase(&mut cx);
        dispatch::drive(&mut cx, EngineOutcome::Done)
    };

    // The open turn surfaces its action menu as AwaitingInput (2b, #447).
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    // Top frame is the InvestigatorTurn for investigator 1...
    assert_eq!(
        state.continuations.top(),
        Some(&Continuation::InvestigatorTurn(InvestigatorTurnFrame {
            investigator: InvestigatorId(1),
            ending: false,
        })),
    );
    // ...sitting above the still-present InvestigationPhase anchor.
    assert!(state.continuations.iter().any(|c| matches!(
        c,
        Continuation::InvestigationPhase(InvestigationPhaseFrame {
            resume: InvestigationResume::TurnBegins
        })
    )));
}

#[test]
fn mulligan_completion_kicks_off_investigation_phase() {
    // After the last investigator mulligans, setup ends and the
    // Investigation phase begins (Rules Reference p.27: no action
    // windows during setup; the game begins after mulligans).
    // active_investigator defaults to None (set when the phase rotates).
    let session = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Investigation)
        .with_turn_order([InvestigatorId(1)])
        .with_mulligan_remaining([InvestigatorId(1)])
        .session()
        // Keep the hand.
        .choose_from_hand(&[]);

    session.assert_at_turn_menu(InvestigatorId(1));
    let state = session.state();
    assert_eq!(
        state.current_mulligan(),
        None,
        "mulligan loop drains once every investigator has mulliganed"
    );
    assert_eq!(
        state.active_investigator,
        Some(InvestigatorId(1)),
        "Investigation phase kicks off and rotates to the lead after mulligan completes"
    );
    // PhaseStarted(Investigation) fires at mulligan completion (not
    // during scenario setup).
    assert_event!(
        session.events(),
        Event::PhaseStarted {
            phase: Phase::Investigation
        }
    );
}

#[test]
fn investigation_anchor_pushed_and_persists_through_turn() {
    // The Investigation driver pushes its anchor at entry; it persists
    // beneath the open-action turn (slice 1a) after the framework windows
    // auto-skip closed.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_turn_order([id])
        .with_phase(Phase::Investigation)
        .build();
    let mut events = Vec::new();
    investigation_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::InvestigationPhase(_))),
        "InvestigationPhase anchor present during the turn; stack = {:?}",
        state.continuations,
    );
}

#[test]
fn investigation_phase_emits_phase_started_and_rotates_to_lead() {
    // Two investigators; investigation_phase should emit
    // PhaseStarted(Investigation), open the post-2.1 InvestigationBegins
    // window (which auto-skips in tests — the test registry serves no ability),
    // and then rotate to the first investigator in turn_order
    // (Rules Reference p.24 step 2.1 → window → step 2.2 lead-first).
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_phase(Phase::Mythos)
        .build();

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = investigation_phase(&mut cx);
    drive_phase(&mut cx, outcome);

    assert_eq!(
        state.active_investigator,
        Some(InvestigatorId(1)),
        "investigation_phase must rotate to the lead (first in turn_order)"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        )),
        "PhaseStarted(Investigation) must be emitted"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::ActionsRemainingChanged { .. })),
        "rotate no longer emits ActionsRemainingChanged (actions reset at Upkeep 4.2)"
    );
}

#[test]
fn investigation_phase_with_empty_turn_order_parks() {
    // Degenerate (cannot occur in real gameplay): no investigators.
    // The InvestigationBegins continuation finds no active
    // investigator and PARKS — active stays None, no PhaseEnded, no
    // advance. Locks in the cascade-breaker behavior (see spec
    // "All-eliminated / no-active-investigator handling").
    //
    // Phase starts as Investigation (matching the real call-site
    // shape: step_phase sets state.phase before calling
    // investigation_phase).
    let mut state = GameStateBuilder::default()
        .with_phase(Phase::Investigation)
        .build();

    let mut events = Vec::new();
    investigation_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert_eq!(
        state.active_investigator, None,
        "no investigator to rotate to"
    );
    assert_eq!(state.phase, Phase::Investigation, "phase must not advance");
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Investigation
            }
        )),
        "parking must NOT end the phase (auto-advancing would loop the round)"
    );
}

#[test]
fn investigation_phase_skips_defeated_lead_and_picks_first_active() {
    // Investigator 1 (lead) is Defeated; investigator 2 is Active.
    // investigation_phase must skip Id(1) and rotate to Id(2).
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_phase(Phase::Mythos)
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = investigation_phase(&mut cx);
    drive_phase(&mut cx, outcome);

    assert_eq!(
        state.active_investigator,
        Some(InvestigatorId(2)),
        "investigation_phase must skip the Defeated lead and rotate to the first Active investigator"
    );
}

#[test]
fn end_turn_for_last_investigator_ends_phase_and_steps_to_enemy() {
    // Single investigator ends their turn: TurnEnded (2.2.2), then
    // PhaseEnded(Investigation) (2.3) from investigation_phase_end,
    // then the cascade enters the Enemy phase.
    let session = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn);
    let (state, events) = (session.state(), session.events());

    // Single investigator with no enemies: the round-ending EndTurn
    // cascades Investigation → Enemy → Upkeep → Mythos and pauses at the
    // step-1.4 encounter-draw prompt.
    assert_eq!(session.prompt().target, Some(OptionTarget::EncounterDeck));
    assert!(
        events.iter().any(
            |e| matches!(e, Event::TurnEnded { investigator } if *investigator == InvestigatorId(1))
        ),
        "step 2.2.2 emits TurnEnded"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Investigation
            }
        )),
        "step 2.3 emits PhaseEnded(Investigation) via investigation_phase_end"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Investigation
                }
            ))
            .count(),
        1,
        "exactly one PhaseEnded(Investigation) — step_phase must not also emit it"
    );
    assert_ne!(
        state.phase,
        Phase::Investigation,
        "phase advanced past Investigation"
    );
}

#[test]
fn end_turn_rotates_to_next_active_and_opens_turn_window() {
    // Two investigators: ending #1's turn returns to 2.2 for #2 and
    // opens the InvestigatorTurnBegins window for them.
    let session = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn);
    let (state, events) = (session.state(), session.events());

    session.assert_at_turn_menu(InvestigatorId(2));
    assert_eq!(
        state.active_investigator,
        Some(InvestigatorId(2)),
        "rotates to the next active investigator (return to 2.2)"
    );
    assert_eq!(
        state.phase,
        Phase::Investigation,
        "phase does not end mid-round"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Investigation
            }
        )),
        "phase must not end while an investigator is still to take a turn"
    );
}

#[test]
fn step_phase_emits_no_phase_ended() {
    // step_phase no longer emits PhaseEnded for any phase — each
    // phase's *_end helper owns it. Direct Investigation→Enemy step:
    // step_phase must NOT emit PhaseEnded(Investigation); the
    // downstream cascade may emit PhaseEnded for Enemy/Upkeep via
    // their own *_end helpers, but that's correct and expected.
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_turn_order([InvestigatorId(1)])
        .with_phase(Phase::Investigation)
        .build();

    let mut events = Vec::new();
    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::PhaseEnded { phase: Phase::Investigation }))
            .count(),
        0,
        "step_phase must emit no PhaseEnded(Investigation) — investigation_phase_end owns it. events = {events:?}"
    );
}

#[test]
fn investigation_entry_emits_phase_started_then_windows_then_lead_active() {
    // Round ≥2 entry via step_phase (Mythos→Investigation) auto-skips
    // both windows (test registry → nothing Fast-eligible) and lands
    // the lead active, with no PhaseEnded yet.
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_turn_order([InvestigatorId(1)])
        .with_phase(Phase::Mythos)
        .build();

    let mut events = Vec::new();
    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Mythos→Investigation

    assert_eq!(state.phase, Phase::Investigation);
    assert_eq!(state.active_investigator, Some(InvestigatorId(1)));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::PhaseStarted {
            phase: Phase::Investigation
        }
    )));
    assert!(!events.iter().any(|e| matches!(
        e,
        Event::PhaseEnded {
            phase: Phase::Investigation
        }
    )));
}
