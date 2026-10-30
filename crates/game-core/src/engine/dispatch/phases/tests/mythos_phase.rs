use super::*;
use crate::assert_event;
use crate::engine::dispatch;
use crate::engine::outcome::InputKind;
use crate::state::{GameStateBuilder, InvestigatorId, Phase, Status};
use crate::test_support::{self, TestSession, TEST_REGISTRIES};

#[test]
fn mythos_phase_emits_phase_started_and_prompts_first_drawer() {
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_phase(Phase::Mythos)
        .with_seating([InvestigatorId(1), InvestigatorId(2)])
        .build();
    let mut events = Vec::new();

    // mythos_phase parks the anchor at Draws; the 1.4 draws run when
    // the drive loop processes that anchor (no agenda deck ⇒ no advance to
    // wait on, so this completes in the same drive).
    mythos_phase(&mut Cx::for_test(&mut state, &mut events));
    let outcome = dispatch::drive(
        &mut Cx::for_test(&mut state, &mut events),
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
    // A MythosPhase{Entry} anchor advanced by `drive` runs the phase opening
    // (PhaseStarted + round bump + push the EncounterDraw loop) and suspends
    // at the first drawer prompt.
    let session = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::Entry,
        })
        .with_seating([InvestigatorId(1)])
        .session();
    // The loop advances the Entry anchor and suspends at the draw prompt.
    session.assert_at_encounter_draw();
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
    // encounter-draw loop while the phase is suspended.
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_seating([InvestigatorId(1)])
        .build();
    let mut events = Vec::new();
    mythos_phase(&mut Cx::for_test(&mut state, &mut events));
    // Draws run from the anchor's Draws resume; drive to reach them.
    let outcome = dispatch::drive(
        &mut Cx::for_test(&mut state, &mut events),
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
fn mythos_phase_with_empty_seating_opens_after_draws_window_inline() {
    let mut state = GameStateBuilder::default()
        .with_phase(Phase::Mythos)
        .build();
    let mut events = Vec::new();

    // No drawers → MythosAfterDraws auto-skips, mythos_phase_end pushes the
    // Investigation anchor; `drive` then advances it — completing the
    // Mythos→Investigation transition.
    let mut cx = Cx::for_test(&mut state, &mut events);
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
    // mythos_phase_end runs only with the MythosPhase anchor on top — it
    // pops the anchor as its first act.
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_seating([InvestigatorId(1)])
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .build();
    let mut events = Vec::new();

    // mythos_phase_end pops the Mythos anchor + pushes the Investigation
    // anchor (Entry); `drive` advances it to run investigation_phase —
    // completing the transition.
    let mut cx = Cx::for_test(&mut state, &mut events);
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

/// Rules Reference p.10: when the lead is eliminated, `mythos_phase` seeds
/// the encounter-draw queue from the first Active investigator in player
/// order rather than blindly taking the stored lead.
#[test]
fn mythos_phase_skips_eliminated_lead_when_seeding_queue() {
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_phase(Phase::Mythos)
        .with_seating([InvestigatorId(1), InvestigatorId(2)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;
    let mut events = Vec::new();

    mythos_phase(&mut Cx::for_test(&mut state, &mut events));
    // The 1.4 draws (queue seeding) run from the Draws anchor; drive.
    dispatch::drive(
        &mut Cx::for_test(&mut state, &mut events),
        EngineOutcome::Done,
    );

    assert_eq!(
        state.current_encounter_drawer(),
        Some(InvestigatorId(2)),
        "the queue must prompt the first Active investigator, not the Defeated lead"
    );
}

/// All investigators in `seating` are eliminated. `mythos_phase`
/// must treat this the same as an empty `seating`: seed to None
/// and open `MythosAfterDraws` inline, which auto-skips and drives
/// `mythos_phase_end`, transitioning to Investigation.
///
/// This is the non-empty-`seating` analogue of
/// `mythos_phase_with_empty_seating_opens_after_draws_window_inline`.
#[test]
fn mythos_phase_with_all_investigators_eliminated_opens_after_draws_window() {
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_phase(Phase::Mythos)
        .with_seating([InvestigatorId(1)])
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .status = Status::Defeated;
    let mut events = Vec::new();

    mythos_phase(&mut Cx::for_test(&mut state, &mut events));
    // No Active drawers: the Draws anchor opens + auto-skips MythosAfterDraws,
    // cascading to Investigation — all once the loop processes it.
    dispatch::drive(
        &mut Cx::for_test(&mut state, &mut events),
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
    let state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2).status(Status::Defeated))
        .with_seat(test_support::seat(3))
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_seating([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
        .with_mythos_draw_remaining([InvestigatorId(1), InvestigatorId(2), InvestigatorId(3)])
        // The card inv1 draws.
        .with_encounter_deck([test_support::TEST_TREACHERY])
        .build();

    // inv1 draws: the queue drops inv1 and must skip the Defeated inv2,
    // landing on inv3.
    let session = TestSession::new(state, TEST_REGISTRIES);
    assert_eq!(
        session.state().current_encounter_drawer(),
        Some(InvestigatorId(1))
    );
    let session = session.confirm();

    // A remaining drawer re-prompts.
    session.assert_at_encounter_draw();
    let state = session.state();
    assert_eq!(
        state.current_encounter_drawer(),
        Some(InvestigatorId(3)),
        "the queue must skip the Defeated inv2 and land on Active inv3"
    );
}
