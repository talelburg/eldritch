use super::*;

/// The one variant whose bucket splits on a field rather than on the
/// variant: a reaction window is an *opportunity* the ended scenario
/// cancels, while the forced run at the same timing point is *mandatory
/// resolution* that completes (ADR 0004). They are the two halves of one
/// `queue_event`, so a `matches!` on the variant alone would get one wrong.
#[test]
fn a_reaction_window_is_cancelled_but_its_forced_run_twin_completes() {
    let window = |mode| Continuation::TimingPointWindow {
        event: TimingEvent::GameEnd,
        bucket: EventTiming::After,
        mode,
        candidates: Vec::new(),
    };
    assert!(
        window(TimingMode::Reaction).cancelled_by_scenario_end(),
        "a reaction window must not open once the scenario has ended"
    );
    assert!(
        !window(TimingMode::Forced).cancelled_by_scenario_end(),
        "the forced ordering run carries mandatory abilities and completes"
    );
}

#[test]
fn the_ending_frame_is_inert_and_survives_its_own_cancellation_pass() {
    let f = Continuation::ScenarioEnd {
        step: ScenarioEndStep::EmitGameEnd,
    };
    assert!(
        !f.cancelled_by_scenario_end(),
        "the ending frame must not cancel itself"
    );
    assert!(
        !f.awaits_input(),
        "the acknowledge above the ending is the prompt, not the ending"
    );
    assert!(!f.is_phase_anchor());
    assert!(
        !f.is_queued_ability(),
        "the ending frame legitimately sits beneath a phase anchor until the \
         anchor is cancelled, so the #569 backstop must not flag it"
    );
}
