//! #764: a defeated active investigator's turn ends (RR Appendix II
//! 2.2.1 → 2.2.2). These cover the arming; `crates/cards/tests/
//! elimination_ends_turn.rs` drives the rotation the flag triggers
//! through the real `apply` loop.

use super::*;

fn turn_frame(state: &GameState) -> Option<(InvestigatorId, bool)> {
    state.continuations.iter().rev().find_map(|c| match c {
        Continuation::InvestigatorTurn {
            investigator,
            ending,
        } => Some((*investigator, *ending)),
        _ => None,
    })
}

#[test]
fn defeat_during_your_own_turn_arms_the_turn_frame_and_announces_the_end() {
    let dead = InvestigatorId(1);
    let mut state = two_investigator_open_turn(dead);
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Damage,
    );

    assert_eq!(
        turn_frame(&state),
        Some((dead, true)),
        "RR 2.2.1: an eliminated investigator cannot take an action, so the turn goes to 2.2.2"
    );
    assert_event!(events, Event::TurnEnded { investigator } if *investigator == dead);
    assert_eq!(
        state.investigators[&dead].actions_remaining, 2,
        "the action that killed them stays charged — unlike `end_turn`, this does not drain"
    );
}

#[test]
fn defeat_outside_your_turn_leaves_the_active_investigators_frame_alone() {
    // Dynamite Blast 01024 catching a co-located investigator, or an Enemy
    // phase attack: the turn frame belongs to someone else and must not be
    // armed — arming it would end the *survivor's* turn.
    let (active, dead) = (InvestigatorId(1), InvestigatorId(2));
    let mut state = two_investigator_open_turn(active);
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Damage,
    );

    assert_eq!(
        turn_frame(&state),
        Some((active, false)),
        "the active investigator's turn is untouched by someone else's defeat"
    );
    assert_no_event!(events, Event::TurnEnded { .. });
}

#[test]
fn defeat_with_no_turn_frame_at_all_is_a_no_op() {
    // The Mythos phase (Rotting Remains 01163) and the Enemy phase: no
    // `InvestigatorTurn` frame exists, so there is nothing to end.
    let dead = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([dead])
        .build();
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Horror,
    );

    assert_eq!(turn_frame(&state), None);
    assert_no_event!(events, Event::TurnEnded { .. });
}

#[test]
fn defeat_while_the_turn_is_already_ending_does_not_re_announce_it() {
    // The player submitted `EndTurn` and a suspending `EndOfTurn` forced
    // ability (Frozen in Fear 01164's willpower test) killed them. `end_turn`
    // already emitted `TurnEnded`; the frame stays armed exactly once and the
    // end is announced exactly once.
    let dead = InvestigatorId(1);
    let mut state = two_investigator_open_turn(dead);
    for c in &mut state.continuations {
        if let Continuation::InvestigatorTurn { ending, .. } = c {
            *ending = true;
        }
    }
    let mut events = Vec::new();

    apply_investigator_elimination(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        dead,
        EliminationCause::Damage,
    );

    assert_eq!(turn_frame(&state), Some((dead, true)));
    assert_no_event!(events, Event::TurnEnded { .. });
}

#[test]
fn resigning_during_your_own_turn_ends_it_too() {
    // RR "Resign": an investigator who resigns "is eliminated by
    // resignation" and "is not considered to have been defeated" — but
    // eliminated all the same, so 2.2.1 sends the turn to 2.2.2 either way.
    //
    // Live since #644: `resign_investigator` is the producer, reached from
    // the Parlor 01115's **Resign** designator. The whole trail through a real
    // activation is `crates/cards/tests/resign.rs`; this pins the turn-end
    // half at the unit seam.
    let resigner = InvestigatorId(1);
    let mut state = two_investigator_open_turn(resigner);
    let mut events = Vec::new();

    resign_investigator(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        resigner,
    );

    assert_eq!(state.investigators[&resigner].status, Status::Resigned);
    assert_eq!(turn_frame(&state), Some((resigner, true)));
}
