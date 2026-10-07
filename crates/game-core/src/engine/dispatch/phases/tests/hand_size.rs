use super::*;
use crate::engine::dispatch;
use crate::engine::outcome::OptionId;
use crate::state::{CardCode, GameStateBuilder, InvestigatorId, UpkeepPhaseFrame};
use crate::{assert_no_event, test_support};

#[test]
fn over_cap_investigators_lists_only_over_eight_in_player_order() {
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_turn_order([inv2, inv1]) // player order: inv2 first
        .build();
    // inv1: 9 cards (over), inv2: 8 cards (at cap, not over).
    state.investigators.get_mut(&inv1).unwrap().hand = vec![CardCode("x".into()); 9];
    state.investigators.get_mut(&inv2).unwrap().hand = vec![CardCode("x".into()); 8];

    assert_eq!(over_cap_investigators(&state), vec![inv1]);

    // Push inv2 over too: order must follow turn_order (inv2 then inv1).
    state.investigators.get_mut(&inv2).unwrap().hand = vec![CardCode("x".into()); 10];
    assert_eq!(over_cap_investigators(&state), vec![inv2, inv1]);
}

#[test]
fn upkeep_anchor_present_while_suspended_at_hand_size() {
    // upkeep_phase pushes the UpkeepPhase anchor at entry; it sits beneath
    // the step-4.5 hand-size discard while the phase is suspended (slice 1a).
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode("x".into()); 10];
    inv.deck = vec![CardCode("y".into())]; // step 4.4 draws 1
    let mut state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let entry = upkeep_phase(&mut cx);
    let outcome = drive_phase(&mut cx, entry);
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "suspends at step 4.5 hand-size discard; got {outcome:?}",
    );
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::UpkeepPhase(_))),
        "UpkeepPhase anchor present while suspended; stack = {:?}",
        state.continuations,
    );
}

#[test]
fn check_hand_size_suspends_for_over_cap_investigator() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        .build();
    state.investigators.get_mut(&id).unwrap().hand = vec![CardCode("x".into()); 10];

    let mut events = Vec::new();
    let outcome = check_hand_size(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "over-cap investigator must suspend; got {outcome:?}"
    );
    assert_eq!(
        state.continuations.iter().rev().find_map(|c| match c {
            Continuation::HandSizeDiscard(p) => Some(p.remaining.clone()),
            _ => None,
        }),
        Some(vec![id]),
    );
}

#[test]
fn check_hand_size_is_noop_when_all_at_or_below_cap() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        .build();
    state.investigators.get_mut(&id).unwrap().hand = vec![CardCode("x".into()); 8];

    let mut events = Vec::new();
    let outcome = check_hand_size(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert_eq!(outcome, EngineOutcome::Done);
    assert!(!matches!(
        state.continuations.top(),
        Some(Continuation::HandSizeDiscard(_))
    ));
}

#[test]
fn upkeep_resume_parks_at_hand_size_discard() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        .build();
    // 13 cards in hand after the step-4.4 draw, still above the 8-card cap;
    // a small deck so the draw doesn't deck out.
    state.investigators.get_mut(&id).unwrap().hand =
        (0..12).map(|i| CardCode(format!("h{i}"))).collect();
    state.investigators.get_mut(&id).unwrap().deck =
        (0..3).map(|i| CardCode(format!("d{i}"))).collect();

    let mut events = Vec::new();
    let outcome = upkeep_resume(&mut Cx {
        state: &mut state,
        events: &mut events,
    });

    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(matches!(
        state.continuations.top(),
        Some(Continuation::HandSizeDiscard(_))
    ));
    assert_eq!(
        state.phase,
        Phase::Upkeep,
        "4.6 must NOT have run while parked"
    );
    assert_no_event!(
        events,
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
}

#[test]
fn resume_hand_size_discard_discards_overflow_and_advances_to_mythos() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        // UpkeepPhase anchor (slice 1a) sits beneath the staged hand-size
        // discard; the round-end teardown pops it.
        .with_phase_anchor(UpkeepPhaseFrame {
            resume: UpkeepResume::Begins,
        })
        .with_hand_size_discard_pending([id])
        .build();
    // 10-card hand: discard exactly 2 (indices 0 and 1) → land at 8.
    state.investigators.get_mut(&id).unwrap().hand =
        (0..10).map(|i| CardCode(format!("c{i}"))).collect();

    let mut events = Vec::new();
    let outcome = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let o = resume_hand_size_discard(
            &mut cx,
            &InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(1)],
            },
        );
        dispatch::drive(&mut cx, o) // slice 1b: loop-driven Upkeep→Mythos
    };

    // The discard drains the hand-size queue, so 4.6 runs and the cascade
    // steps into Mythos, pausing at the step-1.4 encounter-draw prompt.
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(!matches!(
        state.continuations.top(),
        Some(Continuation::HandSizeDiscard(_))
    ));
    assert_eq!(state.investigators[&id].hand.len(), 8);
    assert_eq!(state.investigators[&id].discard.len(), 2);
    assert_eq!(
        state.phase,
        Phase::Mythos,
        "queue drained → 4.6 runs → next round Mythos"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                Event::CardDiscarded {
                    from: Zone::Hand,
                    ..
                }
            ))
            .count(),
        2,
    );
    assert_eq!(
        state.investigators[&id].discard,
        vec![CardCode("c0".into()), CardCode("c1".into())],
        "the cards at the submitted indices (0,1) must be the ones discarded",
    );
}

#[test]
fn resume_hand_size_discard_rejects_wrong_count() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        .with_hand_size_discard_pending([id])
        .build();
    state.investigators.get_mut(&id).unwrap().hand =
        (0..10).map(|i| CardCode(format!("c{i}"))).collect();

    let mut events = Vec::new();
    // Need to discard 2; submitting 1 must reject with state untouched.
    let outcome = resume_hand_size_discard(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &InputResponse::PickMultiple {
            selected: vec![OptionId(0)],
        },
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        state.investigators[&id].hand.len(),
        10,
        "rejected: hand untouched"
    );
    assert!(
        matches!(
            state.continuations.top(),
            Some(Continuation::HandSizeDiscard(_))
        ),
        "rejected: still pending"
    );
    assert!(events.is_empty(), "rejected: no events");
}

#[test]
fn resume_hand_size_discard_rejects_duplicate_and_oob_indices() {
    let id = InvestigatorId(1);
    let build = || {
        let mut s = GameStateBuilder::new()
            .with_investigator(test_support::test_investigator(1))
            .with_turn_order([id])
            .with_phase(Phase::Upkeep)
            .with_hand_size_discard_pending([id])
            .build();
        s.investigators.get_mut(&id).unwrap().hand =
            (0..10).map(|i| CardCode(format!("c{i}"))).collect();
        s
    };

    // Duplicate index (count is 2 but both point at slot 0).
    let mut s1 = build();
    let mut e1 = Vec::new();
    let o1 = resume_hand_size_discard(
        &mut Cx {
            state: &mut s1,
            events: &mut e1,
        },
        &InputResponse::PickMultiple {
            selected: vec![OptionId(0), OptionId(0)],
        },
    );
    assert!(matches!(o1, EngineOutcome::Rejected { .. }));
    assert_eq!(s1.investigators[&id].hand.len(), 10);

    // Out-of-bounds index.
    let mut s2 = build();
    let mut e2 = Vec::new();
    let o2 = resume_hand_size_discard(
        &mut Cx {
            state: &mut s2,
            events: &mut e2,
        },
        &InputResponse::PickMultiple {
            selected: vec![OptionId(0), OptionId(99)],
        },
    );
    assert!(matches!(o2, EngineOutcome::Rejected { .. }));
    assert_eq!(s2.investigators[&id].hand.len(), 10);
}

#[test]
fn resume_hand_size_discard_sequences_investigators_in_player_order() {
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_turn_order([inv1, inv2])
        .with_phase(Phase::Upkeep)
        .with_hand_size_discard_pending([inv1, inv2])
        .build();
    state.investigators.get_mut(&inv1).unwrap().hand =
        (0..9).map(|i| CardCode(format!("a{i}"))).collect(); // discard 1
    state.investigators.get_mut(&inv2).unwrap().hand =
        (0..9).map(|i| CardCode(format!("b{i}"))).collect(); // discard 1

    // inv1 resolves first → still pending for inv2, phase still Upkeep.
    let mut events = Vec::new();
    let o1 = resume_hand_size_discard(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &InputResponse::PickMultiple {
            selected: vec![OptionId(0)],
        },
    );
    assert!(matches!(o1, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        state.continuations.iter().rev().find_map(|c| match c {
            Continuation::HandSizeDiscard(p) => Some(p.remaining.clone()),
            _ => None,
        }),
        Some(vec![inv2]),
    );
    assert_eq!(state.phase, Phase::Upkeep);
    assert_eq!(state.investigators[&inv1].hand.len(), 8);
}

#[test]
fn resume_hand_size_discard_rejects_wrong_response_kind() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        .with_hand_size_discard_pending([id])
        .build();
    state.investigators.get_mut(&id).unwrap().hand =
        (0..10).map(|i| CardCode(format!("c{i}"))).collect();

    let mut events = Vec::new();
    let outcome = resume_hand_size_discard(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &InputResponse::Skip,
    );

    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(
        state.investigators[&id].hand.len(),
        10,
        "rejected: hand untouched"
    );
    assert!(
        matches!(
            state.continuations.top(),
            Some(Continuation::HandSizeDiscard(_))
        ),
        "rejected: still pending"
    );
    assert!(events.is_empty(), "rejected: no events");
}
