use super::*;
use crate::state::{FastWindowKind, GameStateBuilder, MythosResume, PhaseStep};
use crate::test_support;

#[test]
fn returns_false_when_no_investigators() {
    let state = GameStateBuilder::default().build();
    assert!(!any_fast_play_eligible(&state));
}

#[test]
fn returns_false_when_hands_and_in_play_empty() {
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .build();
    assert!(!any_fast_play_eligible(&state));
}

#[test]
fn open_fast_window_with_no_eligibility_auto_skips_inline() {
    // No reactions, no Fast-eligible cards → auto-skip: window
    // opens and closes without ever landing on state.open_windows.
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        // The MythosAfterDraws window now closes onto the MythosPhase anchor
        // (slice 1a); stage it so the auto-skip continuation has its frame.
        .with_phase_anchor(Continuation::MythosPhase {
            resume: MythosResume::AfterDraws,
        })
        .build();
    let mut events = Vec::new();
    open_fast_window(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        FastWindowKind::Phase(PhaseStep::MythosAfterDraws),
    );

    assert!(
        state.open_windows().is_empty(),
        "auto-skip must not leave the window on the stack"
    );
}

/// With no fast-playable card or 0-cost ability available, the enumeration is
/// empty (the auto-skip path). The positive case — a real Fast card becoming
/// a `PlayCard` candidate — is covered by the Task 5 integration regression,
/// because game-core's test registry exposes no playable cards.
#[test]
fn enumerate_fast_plays_empty_when_nothing_eligible() {
    let inv = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(inv)
        .with_investigator(test_support::test_investigator(1))
        .build();
    assert!(enumerate_fast_plays(&state).is_empty());
}
