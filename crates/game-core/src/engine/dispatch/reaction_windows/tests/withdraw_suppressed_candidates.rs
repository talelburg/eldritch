use super::*;
use crate::state::{CardInstanceId, GameStateBuilder, LocationId, TimingSub};
use crate::test_support;

const INV: InvestigatorId = InvestigatorId(1);
/// Deliberately resolved by no registry — these tests install none. The
/// withdrawal sweep moves candidates between frames and never looks a code
/// up. The prefix is this module's, per ADR 0016.
const CODE: &str = "_rw_reaction";

fn discovery() -> TimingEvent {
    TimingEvent::DiscoverClues {
        investigator: INV,
        location: LocationId(10),
        count: 1,
    }
}

/// A one-candidate **reaction** window on `bucket`. The common case; see
/// [`state_with_window_of`] for the other axes.
fn state_with_window(bucket: EventTiming, prevented: bool) -> GameState {
    state_with_window_of(discovery(), bucket, TimingMode::Reaction, prevented)
}

/// A one-candidate window over `event`, in `bucket`, of `mode`, on top of a
/// minimal state whose `pending_cancellation` is `prevented`.
fn state_with_window_of(
    event: TimingEvent,
    bucket: EventTiming,
    mode: TimingMode,
    prevented: bool,
) -> GameState {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(INV)
        .build();
    state.pending_cancellation = prevented;
    state.continuations.push(Continuation::TimingPointWindow {
        event,
        bucket,
        mode,
        candidates: vec![ResolutionCandidate::new(
            CardCode::new(CODE),
            INV,
            AbilityAddress::Printed(0),
            CandidateSource::Ability(AbilitySource::InPlay(CardInstanceId(1))),
        )],
    });
    state
}

fn withdraw(state: &mut GameState) -> (usize, Vec<Event>) {
    let mut events = Vec::new();
    let n = withdraw_suppressed_candidates(&mut Cx {
        state,
        events: &mut events,
    });
    (n, events)
}

fn remaining(state: &GameState) -> usize {
    state
        .continuations
        .last()
        .and_then(Continuation::pending_candidates)
        .expect("the window is still on top")
        .len()
}

/// The Dodge 01023 shape: the condition was prevented in this very cell, so
/// the sibling still pending in the window is withdrawn rather than offered.
#[test]
fn a_prevented_condition_empties_its_when_cell_window() {
    let mut state = state_with_window(EventTiming::When, true);
    let (n, events) = withdraw(&mut state);
    assert_eq!(n, 1, "the pending sibling must be withdrawn");
    assert_eq!(remaining(&state), 0);
    crate::assert_event!(
        events,
        Event::ReactionOptionLapsed {
            reason: LapseReason::ConditionPrevented,
            ..
        }
    );
}

/// No signal, no suppression — the ordinary window is left entirely alone
/// (its own re-scan withdrawal is `withdraw_lapsed_candidates`' job).
#[test]
fn an_unprevented_condition_withdraws_nothing() {
    let mut state = state_with_window(EventTiming::When, false);
    let (n, events) = withdraw(&mut state);
    assert_eq!(n, 0);
    assert_eq!(remaining(&state), 1);
    assert!(events.is_empty(), "no lapse to report; events = {events:?}");
}

/// Scoped to the `when` cell: it is the only one whose abilities resolve
/// before the condition does, so a signal seen at an `at` or `after` window
/// belongs to something else and must not empty it.
#[test]
fn a_later_cells_window_is_untouched() {
    for bucket in [EventTiming::At, EventTiming::After] {
        let mut state = state_with_window(bucket, true);
        let (n, _) = withdraw(&mut state);
        assert_eq!(n, 0, "{bucket:?} must not be suppressed");
        assert_eq!(remaining(&state), 1);
    }
}

/// The forced arm: a 2+ lead-ordered run (#213) empties the same way, so it
/// closes itself instead of demanding a pick for a condition that is no
/// longer happening. The unit half of the shape Dodge 01023's ruling is
/// stated about; the corpus half — dodging Silver Twilight Acolyte 01102 —
/// is `crates/cards/tests/dodge.rs`.
#[test]
fn a_forced_run_in_the_when_cell_empties_too() {
    let mut state = state_with_window_of(discovery(), EventTiming::When, TimingMode::Forced, true);
    let (n, events) = withdraw(&mut state);
    assert_eq!(
        n, 1,
        "a forced run is withdrawn from like a reaction window"
    );
    assert_eq!(remaining(&state), 0);
    crate::assert_event!(
        events,
        Event::ReactionOptionLapsed {
            reason: LapseReason::ConditionPrevented,
            ..
        }
    );
}

/// A **caller-owned** condition is left alone: it has already mutated the
/// board by the time any window opens, so a prevention signal in flight is
/// not its to consume. The enemy attack used to be this fixture and #704
/// migrated it; the soak condition stood in until #727 replaced it with the
/// coordinator-owned `DamageAssigned`/`DamagePlaced` pair, so `EnteredPlay`
/// (Research Librarian 01032's window) stands in now.
#[test]
fn a_caller_owned_conditions_window_is_untouched() {
    let entered = TimingEvent::EnteredPlay {
        instance: CardInstanceId(1),
        controller: INV,
    };
    debug_assert!(matches!(
        entered.condition_resolution(),
        ConditionResolution::Caller
    ));
    let mut state = state_with_window_of(entered, EventTiming::When, TimingMode::Reaction, true);
    let (n, events) = withdraw(&mut state);
    assert_eq!(n, 0, "a caller-owned condition is not suppressed here");
    assert_eq!(remaining(&state), 1);
    assert!(events.is_empty(), "no lapse to report; events = {events:?}");
}

/// A frame that is not a resolution window at all — here the `when` cell's
/// own [`Continuation::TimingPoint`], which is what the stack holds while a
/// single forced ability resolves inline. No candidates to withdraw, so the
/// callers need no guard of their own.
#[test]
fn a_non_window_frame_is_a_no_op() {
    let mut state = GameStateBuilder::default().build();
    state.pending_cancellation = true;
    state.continuations.push(Continuation::TimingPoint {
        event: discovery(),
        bucket: EventTiming::When,
        sub: TimingSub::Reaction,
    });
    let (n, events) = withdraw(&mut state);
    assert_eq!(n, 0);
    assert!(events.is_empty());
}
