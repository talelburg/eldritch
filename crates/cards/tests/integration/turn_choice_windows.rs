//! The lead's choice of who takes each Investigation turn keeps its place
//! between the phase's player windows.
//!
//! Appendix II 2.2: *"The investigators choose among themselves who (among the
//! investigators) will take this turn, and making this choice begins that
//! investigator's turn."* So the post-2.1 player window comes before the
//! round's first choice, and the post-2.2 window comes after the chosen turn
//! begins.
//!
//! Beat Cop 01018's *"\[fast\] Discard Beat Cop: Deal 1 damage to an enemy at
//! your location."*, with an enemy beside it, keeps each window standing so a
//! test can see which one is open.

use cards::TEST_REGISTRIES;
use game_core::state::{
    Continuation, EnemyId, FastWindowFrame, FastWindowKind, GameStateBuilder, InvestigatorId,
    LocationId, PhaseStep,
};
use game_core::test_support::{self, seat, BoardLookup as _, TestSession};

/// Beat Cop 01018.
const BEAT_COP: &str = "01018";

const A: InvestigatorId = InvestigatorId(1);
const B: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(101);
const GHOUL: EnemyId = EnemyId(100);

/// Two seats `[A, B]` at the start of the Investigation phase, step 2.1's
/// forced abilities resolved, with B's Beat Cop and an enemy beside it.
fn investigation_begins() -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(seat(1).at(HERE))
        .with_seat(seat(2).at(HERE).in_play([BEAT_COP]))
        .with_seating([A, B])
        .with_enemy(test_support::enemy(GHOUL).health(3).at(HERE))
        .beginning_investigation_phase()
        .session()
}

/// Assert `session` rests at the framework player window for `step`, offering
/// B's Beat Cop.
#[track_caller]
fn assert_at_window(session: &TestSession, step: PhaseStep) {
    assert!(
        matches!(
            session.state().continuations.top(),
            Some(Continuation::FastWindow(FastWindowFrame { kind, .. }))
                if *kind == FastWindowKind::Phase(step)
        ),
        "expected the {step:?} window; the top frame is {:?}",
        session.state().continuations.top(),
    );
    assert_eq!(session.offered_at(&session.state().card(B, BEAT_COP)), 1);
}

#[test]
fn the_post_2_1_window_comes_before_the_first_choice() {
    let session = investigation_begins();
    assert_at_window(&session, PhaseStep::InvestigationBegins);
    assert_eq!(session.state().active_investigator, None);

    session.skip().choose_turn(B).skip().assert_at_turn_menu(B);
}

#[test]
fn the_post_2_2_window_opens_after_the_chosen_turn_begins() {
    let session = investigation_begins().skip().choose_turn(B);
    assert_at_window(&session, PhaseStep::InvestigatorTurnBegins);
    assert_eq!(session.state().active_investigator, Some(B));

    session.skip().assert_at_turn_menu(B);
}
