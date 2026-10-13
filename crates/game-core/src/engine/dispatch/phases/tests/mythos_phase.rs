use super::*;
use crate::assert_event;
use crate::engine::dispatch;
use crate::engine::outcome::{InputKind, OptionTarget};
use crate::state::{GameStateBuilder, InvestigatorId, Phase, Status};
use crate::test_support::{self, TestSession};

#[test]
fn mythos_phase_emits_phase_started_and_prompts_first_drawer() {
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    let mut events = Vec::new();

    // mythos_phase parks the anchor at Draws (#482); the 1.4 draws run when
    // the drive loop processes that anchor (no agenda deck ⇒ no advance to
    // wait on, so this completes in the same drive).
    mythos_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    let outcome = dispatch::drive(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EngineOutcome::Done,
    );

    let EngineOutcome::AwaitingInput { request, .. } = &outcome else {
        panic!("mythos_phase opens the first encounter-draw prompt, got {outcome:?}");
    };
    // The draw is a binary acknowledge: kind Confirm, not skippable.
    assert_eq!(request.kind, InputKind::Confirm);
    assert!(!request.skippable);
    assert_eq!(state.current_encounter_drawer(), Some(InvestigatorId(1)));
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Mythos
            }
        )),
        "must emit PhaseStarted(Mythos); events = {events:?}"
    );
}

#[test]
fn mythos_drives_from_entry_via_the_loop() {
    // slice 1b: a MythosPhase{Entry} anchor advanced by `drive` runs the
    // phase opening (PhaseStarted + round bump + push the EncounterDraw
    // loop) and suspends at the first drawer prompt — same as the old
    // synchronous mythos_phase entry.
    let session = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::Entry,
        })
        .with_turn_order([InvestigatorId(1)])
        .session();
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::EncounterDeck),
        "the loop advances the Entry anchor and suspends at the draw prompt; got {:?}",
        session.prompt(),
    );
    assert_eq!(
        session.state().current_encounter_drawer(),
        Some(InvestigatorId(1))
    );
    assert_event!(
        session.events(),
        Event::PhaseStarted {
            phase: Phase::Mythos
        }
    );
}

#[test]
fn mythos_anchor_pushed_during_phase() {
    // The Mythos driver pushes its anchor at entry; it sits beneath the
    // encounter-draw loop while the phase is suspended (slice 1a).
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1)])
        .build();
    let mut events = Vec::new();
    mythos_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    // Draws run from the anchor's Draws resume (#482); drive to reach them.
    let outcome = dispatch::drive(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EngineOutcome::Done,
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::MythosPhase(_))),
        "MythosPhase anchor on the stack during the phase; stack = {:?}",
        state.continuations,
    );
}

#[test]
fn mythos_phase_with_empty_turn_order_opens_after_draws_window_inline() {
    let mut state = GameStateBuilder::default()
        .with_phase(Phase::Mythos)
        .build();
    let mut events = Vec::new();

    // No drawers → MythosAfterDraws auto-skips, mythos_phase_end pushes the
    // Investigation anchor; `drive` then advances it (slice 1b) — completing
    // the Mythos→Investigation transition that was synchronous in slice 1a.
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = mythos_phase(&mut cx);
    let outcome = dispatch::drive(&mut cx, outcome);

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.current_encounter_drawer(), None);
    assert_eq!(state.phase, Phase::Investigation);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Mythos
            }
        )),
        "must emit PhaseEnded(Mythos); events = {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        )),
        "must emit PhaseStarted(Investigation); events = {events:?}"
    );
}

#[test]
fn mythos_phase_end_emits_phase_ended_and_steps_to_investigation() {
    // mythos_phase_end now runs only with the MythosPhase anchor on top
    // (slice 1a) — it pops the anchor as its first act.
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1)])
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .build();
    let mut events = Vec::new();

    // mythos_phase_end pops the Mythos anchor + pushes the Investigation
    // anchor (Entry); `drive` advances it to run investigation_phase (slice
    // 1b) — completing the transition.
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    mythos_phase_end(&mut cx);
    let _ = dispatch::drive(&mut cx, EngineOutcome::Done);

    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::MythosPhase(_))),
        "mythos_phase_end pops the Mythos anchor (the cascade into \
         Investigation then pushes its own anchor)",
    );
    assert_eq!(state.phase, Phase::Investigation);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Mythos
            }
        )),
        "must emit PhaseEnded(Mythos); events = {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Investigation
            }
        )),
        "must emit PhaseStarted(Investigation); events = {events:?}"
    );
}

/// Site 1 fix (Rules Reference p.10): when the lead investigator in
/// `turn_order` is eliminated, `mythos_phase` must seed the encounter-draw
/// queue from the first Active investigator rather than blindly taking
/// `turn_order.first()`.
#[test]
fn mythos_phase_skips_eliminated_lead_when_seeding_queue() {
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;
    let mut events = Vec::new();

    mythos_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    // The 1.4 draws (queue seeding) run from the Draws anchor (#482); drive.
    dispatch::drive(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EngineOutcome::Done,
    );

    assert_eq!(
        state.current_encounter_drawer(),
        Some(InvestigatorId(2)),
        "the queue must prompt the first Active investigator, not the Defeated lead"
    );
}

/// All investigators in `turn_order` are eliminated. `mythos_phase`
/// must treat this the same as an empty `turn_order`: seed to None
/// and open `MythosAfterDraws` inline, which auto-skips and drives
/// `mythos_phase_end`, transitioning to Investigation.
///
/// This is the non-empty-`turn_order` analogue of
/// `mythos_phase_with_empty_turn_order_opens_after_draws_window_inline`.
#[test]
fn mythos_phase_with_all_investigators_eliminated_opens_after_draws_window() {
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_turn_order([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;
    let mut events = Vec::new();

    mythos_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    // No Active drawers: the Draws anchor opens + auto-skips MythosAfterDraws,
    // cascading to Investigation — all once the loop processes it (#482).
    dispatch::drive(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        EngineOutcome::Done,
    );

    assert_eq!(state.current_encounter_drawer(), None);
    assert_eq!(
        state.phase,
        Phase::Investigation,
        "no Active drawers → MythosAfterDraws fires inline → Investigation"
    );
}

/// Site 2 fix (Rules Reference p.10): when advancing the encounter-draw
/// queue after a completed draw, eliminated investigators in the middle of
/// the queue must be skipped. Here inv2 is Defeated; the queue must advance
/// from inv1 to inv3.
#[test]
fn advance_encounter_draw_skips_eliminated_middle_investigator() {
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_seat(test_support::seat(3))
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_turn_order([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
        .with_mythos_draw_remaining([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
        .build();
    // A seat has no status setter; inv2 is eliminated on the built board.
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .status = Status::Defeated;
    // The card inv1 draws.
    state
        .encounter_deck
        .push_back(CardCode::new(test_support::TEST_TREACHERY));

    // inv1 draws: the queue drops inv1 and must skip the Defeated inv2,
    // landing on inv3.
    let session = TestSession::new(state);
    assert_eq!(
        session.state().current_encounter_drawer(),
        Some(InvestigatorId(1))
    );
    let session = session.confirm();

    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::EncounterDeck),
        "a remaining drawer re-prompts, got {:?}",
        session.prompt(),
    );
    let state = session.state();
    assert_eq!(
        state.current_encounter_drawer(),
        Some(InvestigatorId(3)),
        "the queue must skip the Defeated inv2 and land on Active inv3"
    );
}

#[test]
fn first_active_investigator_finds_first_active_skipping_eliminated() {
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_seat(test_support::seat(3))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .status = Status::Defeated;

    assert_eq!(
        cursor::first_active_investigator(&state),
        Some(InvestigatorId(3)),
        "first Active in turn_order after skipping eliminated"
    );
}

#[test]
fn first_active_investigator_returns_none_when_all_eliminated() {
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;

    assert_eq!(cursor::first_active_investigator(&state), None);
}

#[test]
fn first_active_investigator_returns_none_when_turn_order_empty() {
    let state = GameStateBuilder::default().build();
    assert_eq!(cursor::first_active_investigator(&state), None);
}

#[test]
fn next_active_investigator_after_skips_eliminated_middle() {
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_seat(test_support::seat(3))
        .with_seat(test_support::seat(4))
        .with_turn_order([
            InvestigatorId(1),
            InvestigatorId(2),
            InvestigatorId(3),
            InvestigatorId(4),
        ])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .status = Status::Defeated;

    assert_eq!(
        cursor::next_active_investigator_after(&state, InvestigatorId(1)),
        Some(InvestigatorId(3)),
        "advance from 1 skips Defeated 2, lands on 3"
    );
    assert_eq!(
        cursor::next_active_investigator_after(&state, InvestigatorId(3)),
        Some(InvestigatorId(4)),
        "advance from 3 lands on 4"
    );
    assert_eq!(
        cursor::next_active_investigator_after(&state, InvestigatorId(4)),
        None,
        "advance past the last entry returns None"
    );
}

#[test]
fn next_active_investigator_after_returns_none_when_current_not_in_turn_order() {
    let state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_turn_order([InvestigatorId(1)])
        .build();

    assert_eq!(
        cursor::next_active_investigator_after(&state, InvestigatorId(99)),
        None
    );
}

#[test]
fn next_active_investigator_after_works_when_current_is_non_active() {
    // Defeated-mid-loop semantics: `current` may be Defeated by the
    // time we advance from them. The cursor still finds the right
    // successor.
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;

    assert_eq!(
        cursor::next_active_investigator_after(&state, InvestigatorId(1)),
        Some(InvestigatorId(2)),
        "current=1 is non-Active but turn_order still anchors the index"
    );
}
