use super::*;
use crate::action::{Action, PlayerAction};
use crate::engine::outcome::{InputKind, OptionId, OptionTarget, PickCount};
use crate::state::{CardCode, GameStateBuilder, InvestigatorId};
use crate::test_support::{self, TestSession, TEST_REGISTRIES};
use crate::{assert_event_count, assert_no_event};

/// `n` distinct hand cards `{prefix}0`, `{prefix}1`, ….
fn hand(prefix: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i}")).collect()
}

/// The session rests at step 4.5's hand-size discard: a hand `PickMultiple`
/// in the Upkeep phase that is not a skill test's commit window.
#[track_caller]
fn assert_at_hand_size_discard(session: &TestSession) {
    assert!(matches!(session.prompt().kind, InputKind::PickMultiple(_)));
    assert!(session.state().current_skill_test().is_none());
    assert_eq!(session.state().phase, Phase::Upkeep);
}

/// Step 4.5's hand-size discard, the moment it is asked: the Upkeep phase
/// ending (after step 4.4's draw) with each of `hands` seated in turn order.
fn ending_upkeep_with_hands(hands: &[(u32, Vec<String>)]) -> TestSession {
    hands
        .iter()
        .fold(
            GameStateBuilder::new().with_registries(TEST_REGISTRIES),
            |builder, (id, hand)| {
                builder.with_seat(test_support::seat(*id).hand(hand.iter().map(String::as_str)))
            },
        )
        .with_turn_order(hands.iter().map(|(id, _)| InvestigatorId(*id)))
        .ending_upkeep_phase()
        .session()
}

/// The discard offers the prompted investigator's hand cards, anchored to
/// them, and asks for exactly the overflow: ten cards, two to discard.
#[test]
fn the_hand_size_discard_offers_the_hand_and_asks_for_exactly_the_overflow() {
    let session = ending_upkeep_with_hands(&[(1, hand("c", 10))]);
    let request = session.prompt();
    let anchors: Vec<_> = request.options.iter().map(|o| o.target.clone()).collect();
    let expected: Vec<_> = (0..10)
        .map(|hand_index| {
            Some(OptionTarget::HandCard {
                investigator: InvestigatorId(1),
                hand_index,
            })
        })
        .collect();
    assert_eq!(anchors, expected);
    assert_eq!(
        request.kind,
        InputKind::PickMultiple(PickCount { min: 2, max: 2 })
    );
}

#[test]
fn over_cap_investigators_lists_only_over_eight_in_player_order() {
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    // inv1: 9 cards (over), inv2: 8 cards (at cap, not over).
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).hand(["x"; 9]))
        .with_seat(test_support::seat(2).hand(["x"; 8]))
        .with_turn_order([inv2, inv1]) // player order: inv2 first
        .build();

    assert_eq!(over_cap_investigators(&state), vec![inv1]);

    // Push inv2 over too: order must follow turn_order (inv2 then inv1).
    state.investigators.get_mut(&inv2).unwrap().hand = vec![CardCode("x".into()); 10];
    assert_eq!(over_cap_investigators(&state), vec![inv2, inv1]);
}

#[test]
fn upkeep_anchor_present_while_suspended_at_hand_size() {
    // upkeep_phase pushes the UpkeepPhase anchor at entry; it sits beneath
    // the step-4.5 hand-size discard while the phase is suspended.
    // The anchor's place on the stack is the subject, so this reads the stack.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        // Step 4.4 draws 1.
        .with_seat(test_support::seat(1).hand(["x"; 10]).deck(["y"]))
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
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
    let session = ending_upkeep_with_hands(&[(1, hand("x", 10))]);

    assert_at_hand_size_discard(&session);
    assert_no_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
    // The prompt is investigator 1's: their cards answer it.
    let session = session.choose_from_hand(&["x0", "x1"]);
    assert_eq!(
        session.state().investigators[&InvestigatorId(1)].hand.len(),
        8
    );
}

#[test]
fn check_hand_size_is_noop_when_all_at_or_below_cap() {
    let session = ending_upkeep_with_hands(&[(1, hand("x", 8))]);

    // Nothing to discard: 4.6 runs and play goes on to the next round's
    // encounter draw.
    session.assert_at_encounter_draw();
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
    assert_eq!(
        session.state().investigators[&InvestigatorId(1)].hand.len(),
        8
    );
}

#[test]
fn upkeep_resume_parks_at_hand_size_discard() {
    let id = InvestigatorId(1);
    // 13 cards in hand after the step-4.4 draw, still above the 8-card cap;
    // a small deck so the draw doesn't deck out.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            test_support::seat(1)
                .hand(hand("h", 12).iter().map(String::as_str))
                .deck(hand("d", 3).iter().map(String::as_str)),
        )
        .with_turn_order([id])
        .with_phase(Phase::Upkeep)
        // The post-4.1 window has closed: steps 4.2–4.6 run next.
        .with_phase_anchor(UpkeepPhaseFrame {
            resume: UpkeepResume::Begins,
        })
        .session();

    assert_at_hand_size_discard(&session);
    assert_eq!(session.state().investigators[&id].hand.len(), 13);
    assert_no_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Upkeep
        }
    );
}

#[test]
fn resume_hand_size_discard_discards_overflow_and_advances_to_mythos() {
    let id = InvestigatorId(1);
    // 10-card hand: discard exactly 2 (c0 and c1) → land at 8.
    let session = ending_upkeep_with_hands(&[(1, hand("c", 10))]).choose_from_hand(&["c0", "c1"]);

    // The discard drains the hand-size queue, so 4.6 runs and the cascade
    // steps into Mythos, pausing at the step-1.4 encounter-draw prompt.
    session.assert_at_encounter_draw();
    let state = session.state();
    assert_eq!(state.investigators[&id].hand.len(), 8);
    assert_eq!(state.investigators[&id].discard.len(), 2);
    assert_eq!(
        state.phase,
        Phase::Mythos,
        "queue drained → 4.6 runs → next round Mythos"
    );
    assert_event_count!(
        session.events(),
        2,
        Event::CardDiscarded {
            from: Zone::Hand,
            ..
        }
    );
    assert_eq!(
        state.investigators[&id].discard,
        vec![CardCode("c0".into()), CardCode("c1".into())],
        "the chosen cards must be the ones discarded",
    );
}

#[test]
fn resume_hand_size_discard_rejects_wrong_count() {
    let id = InvestigatorId(1);
    // Need to discard 2; choosing 1 must reject with state untouched.
    let session = ending_upkeep_with_hands(&[(1, hand("c", 10))]);
    let events_before = session.events().len();
    let session = session.choose_from_hand(&["c0"]);

    session.expect_rejected();
    assert_eq!(
        session.state().investigators[&id].hand.len(),
        10,
        "rejected: hand untouched"
    );
    assert_eq!(session.events().len(), events_before, "rejected: no events");
    // Rejected: still pending — the same prompt takes a correct answer.
    assert_at_hand_size_discard(&session);
    let session = session.choose_from_hand(&["c0", "c1"]);
    assert_eq!(session.state().investigators[&id].hand.len(), 8);
}

#[test]
fn resume_hand_size_discard_rejects_duplicate_and_oob_indices() {
    // The subject is a malformed wire response — hand indices no
    // `choose_from_hand` can name — so the raw `ResolveInput` is sent as is.
    let id = InvestigatorId(1);
    let malformed = |selected: Vec<OptionId>| {
        ending_upkeep_with_hands(&[(1, hand("c", 10))]).apply(Action::Player(
            PlayerAction::ResolveInput {
                response: InputResponse::PickMultiple { selected },
            },
        ))
    };

    // Duplicate index (count is 2 but both point at slot 0).
    let s1 = malformed(vec![OptionId(0), OptionId(0)]);
    s1.expect_rejected();
    assert_eq!(s1.state().investigators[&id].hand.len(), 10);

    // Out-of-bounds index.
    let s2 = malformed(vec![OptionId(0), OptionId(99)]);
    s2.expect_rejected();
    assert_eq!(s2.state().investigators[&id].hand.len(), 10);
}

#[test]
fn resume_hand_size_discard_sequences_investigators_in_player_order() {
    let inv1 = InvestigatorId(1);
    let inv2 = InvestigatorId(2);
    // Each holds 9: discard 1.
    let session = ending_upkeep_with_hands(&[(1, hand("a", 9)), (2, hand("b", 9))]);

    // inv1 resolves first → still pending for inv2, phase still Upkeep.
    let session = session.choose_from_hand(&["a0"]);
    assert_at_hand_size_discard(&session);
    assert_eq!(session.state().investigators[&inv1].hand.len(), 8);
    assert_eq!(session.state().investigators[&inv2].hand.len(), 9);

    // The prompt is now inv2's: their card answers it.
    let session = session.choose_from_hand(&["b0"]);
    assert_eq!(session.state().investigators[&inv2].hand.len(), 8);
}

#[test]
fn resume_hand_size_discard_rejects_wrong_response_kind() {
    // A `Skip` is the wrong kind of answer to a hand-size discard.
    let id = InvestigatorId(1);
    let session = ending_upkeep_with_hands(&[(1, hand("c", 10))]);
    let events_before = session.events().len();
    let session = session.skip();

    session.expect_rejected();
    assert_eq!(
        session.state().investigators[&id].hand.len(),
        10,
        "rejected: hand untouched"
    );
    assert_eq!(session.events().len(), events_before, "rejected: no events");
    // Rejected: still pending.
    assert_at_hand_size_discard(&session);
}
