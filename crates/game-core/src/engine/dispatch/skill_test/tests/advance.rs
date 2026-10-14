use super::*;
use crate::state::ChaosBag;
use crate::test_support::TestSession;
use crate::{assert_event, assert_no_event};

// The former `revelation_skill_test_failure_deals_margin_damage_and_discards`
// unit test is gone (#380): it simulated the removed
// `pending_revelation_discard` slot and drove `finish_skill_test` directly,
// bypassing the new `resolve_input`-chokepoint disposal. The real
// suspended-Revelation-into-skill-test discard is integration-tested by
// `crates/cards/tests/revelation_treacheries.rs::grasping_hands_*`
// (01162), and the margin-damage math by the same test.

/// A plain (non-revelation) skill test disposes of no encounter card — the
/// skill-test driver no longer touches encounter disposal at all (#380).
#[test]
fn plain_skill_test_disposes_of_no_encounter_card() {
    let inv = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .open_turn(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    let session = TestSession::new_with_registries(state, crate::engine::TEST_REGISTRIES)
        .test_skill(inv, SkillKind::Intellect, 1)
        .commit(&[]);
    session.assert_at_turn_menu(inv);
    assert!(session.state().encounter_discard.is_empty());
}

/// A skill test with an `on_success` effect runs it on a successful
/// draw (the success-side mirror of the `on_fail` path).
#[test]
fn skill_test_runs_on_success_effect_on_a_passing_draw() {
    let inv = InvestigatorId(1);
    // Willpower 3 + Numeric(0) = 3 vs difficulty 2 → success.
    let mut state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_active_investigator(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    let out = start_skill_test(
        &mut cx,
        inv,
        SkillKind::Willpower,
        SkillTestKind::Plain,
        DifficultyBasis::Fixed(2),
        SkillTestFollowUp::None,
        Some(dsl::deal_horror(InvestigatorTarget::You, 1u8)),
        None,
        None,
        None,
    );
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    let out = finish_skill_test(&mut cx, &[]);
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&inv].horror(),
        1,
        "on_success effect ran on the passing draw",
    );
}

/// Both ST.1/ST.2 player windows open and auto-skip (no ability served / nothing
/// Fast-eligible), bracketing the commit, and the test still resolves. (#374.)
#[test]
fn skill_test_opens_and_auto_skips_both_player_windows() {
    let inv = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .open_turn(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();

    // start -> PreCommitWindow auto-skips window 1 -> parks at AwaitingCommit.
    let session = TestSession::new_with_registries(state, crate::engine::TEST_REGISTRIES)
        .test_skill(inv, SkillKind::Willpower, 2);
    assert!(
        session.state().current_skill_test().is_some()
            && session.prompt().kind == InputKind::PickMultiple,
        "commit prompt (window 1 before commit opened and auto-skipped to it)"
    );

    // commit nothing -> PreTokenWindow auto-skips window 2 -> resolves to end.
    let session = session.commit(&[]);
    session.assert_at_turn_menu(inv);
    assert_event!(session.events(), Event::SkillTestEnded { .. });
}

/// Closing a skill-test player window re-enters `advance` at the
/// pre-advanced cursor (the `run_fast_continuation` arm), not just via the
/// auto-skip path. Here window 1 is "about to close" — the cursor is already
/// `AwaitingCommit` — so `run_fast_continuation` must re-enter `advance` and
/// emit the commit prompt. (#374.)
#[test]
fn closing_a_skill_test_player_window_re_enters_advance() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_active_investigator(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    // A SkillTest pre-advanced to AwaitingCommit, as if window 1 just opened.
    state
        .continuations
        .push(Continuation::SkillTest(test_support::test_skill_test(
            SkillTestId(0),
            inv,
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
        )));
    let mut events = Vec::new();
    let out = reaction_windows::run_fast_continuation(
        &mut Cx::for_test(&mut state, &mut events),
        FastWindowKind::SkillTest {
            before_token: false,
        },
    );
    let EngineOutcome::AwaitingInput { request, .. } = &out else {
        panic!("expected the commit prompt after the window closed, got {out:?}");
    };
    assert!(
        request.prompt.contains("Commit cards"),
        "re-entered advance at AwaitingCommit: {request:?}",
    );
}

/// The reified driver: `start_skill_test` parks at `AwaitingCommit` via
/// `advance` (emitting the commit prompt), and committing drives the test to
/// teardown — `SkillTestStarted` then `SkillTestEnded`, no frame left behind.
#[test]
fn commit_emits_then_resolves_through_advance() {
    let inv = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .open_turn(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();

    let session = TestSession::new_with_registries(state, crate::engine::TEST_REGISTRIES)
        .test_skill(inv, SkillKind::Willpower, 2);
    // advance parked at AwaitingCommit and emitted the commit prompt.
    assert!(
        session.prompt().prompt.contains("Commit cards"),
        "the AwaitingCommit arm emits the commit prompt: {:?}",
        session.prompt(),
    );
    assert!(session.state().current_skill_test().is_some());

    // Commit nothing → the hop parks; the loop drives to teardown.
    let session = session.commit(&[]);
    session.assert_at_turn_menu(inv);
    assert_event!(session.events(), Event::SkillTestStarted { .. });
    assert_event!(session.events(), Event::SkillTestEnded { .. });
    assert!(
        session.state().current_skill_test().is_none(),
        "the SkillTest frame was torn down",
    );
}

/// Flag on: a skill test pauses at the acknowledge step with a `Confirm`
/// prompt — after the result events are emitted, before teardown — and a
/// Confirm drives it to completion (#478).
#[test]
fn interactive_acknowledge_pauses_for_confirm_then_resolves() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .open_turn(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    state.interactive_acknowledge = true;

    let session = TestSession::new_with_registries(state, crate::engine::TEST_REGISTRIES)
        .test_skill(inv, SkillKind::Willpower, 2);
    assert_eq!(
        session.prompt().kind,
        InputKind::PickMultiple,
        "commit prompt"
    );

    // Commit nothing -> resolution runs, then suspends at the acknowledge step.
    let session = session.commit(&[]);
    let request = session.prompt();
    assert_eq!(
        request.kind,
        InputKind::Confirm,
        "acknowledge is a Confirm prompt"
    );
    // The acknowledge pause is deliberately **un-anchored**: it renders on the
    // skill-test result modal, not on a board surface. This is the half of the
    // ADR-0011 pair that tells it apart from the encounter draw, which is
    // `.at(EncounterDeck)`; the two are otherwise byte-identical on the wire.
    assert_eq!(
        request.target, None,
        "the acknowledge pause carries no board anchor (ADR 0011)"
    );
    // The result is already logged when the player is asked to acknowledge.
    assert_event!(session.events(), Event::ChaosTokenRevealed { .. });
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    // Teardown waits on the acknowledgment.
    assert_no_event!(session.events(), Event::SkillTestEnded { .. });

    // Confirm -> drive into teardown.
    let session = session.confirm();
    session.assert_at_turn_menu(inv);
    assert_event!(session.events(), Event::SkillTestEnded { .. });
}

/// Flag off (default): no acknowledge pause — the test resolves straight
/// through, exactly as before #478 (guards against test churn).
#[test]
fn no_acknowledge_pause_when_flag_off() {
    let inv = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .open_turn(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();

    let session = TestSession::new_with_registries(state, crate::engine::TEST_REGISTRIES)
        .test_skill(inv, SkillKind::Willpower, 2);
    assert_eq!(
        session.prompt().kind,
        InputKind::PickMultiple,
        "commit prompt"
    );
    // No acknowledge pause when the flag is off: straight back to the menu.
    let session = session.commit(&[]);
    session.assert_at_turn_menu(inv);
    assert_event!(session.events(), Event::SkillTestEnded { .. });
}

/// `acknowledge_outcome` rejects (state untouched) when there is no in-flight
/// test to acknowledge.
#[test]
fn acknowledge_outcome_rejects_without_in_flight_test() {
    let mut state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    let out = acknowledge_outcome(&mut cx);
    assert!(matches!(out, EngineOutcome::Rejected { .. }), "got {out:?}");
    assert!(events.is_empty(), "rejection emits no events");
}

/// The commit hop parks the resolution for the loop rather than driving it
/// itself: `finish_skill_test` returns `Done` with the `SkillTest` frame on
/// top at `PreTokenWindow` and emits no `SkillTestEnded`; the `drive` loop's
/// `SkillTest` arm then resolves it to teardown. (Slice C, #431 — commit-hop
/// re-entry retired.)
#[test]
fn finish_skill_test_parks_the_resolution_for_the_loop() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(crate::engine::TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .with_active_investigator(inv)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);

    // Park at AwaitingCommit (the commit prompt).
    let out = start_skill_test(
        &mut cx,
        inv,
        SkillKind::Willpower,
        SkillTestKind::Plain,
        DifficultyBasis::Fixed(2),
        SkillTestFollowUp::None,
        None,
        None,
        None,
        None,
    );
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));

    // Commit nothing: the hop PARKS — it must not itself resolve the test.
    let out = finish_skill_test(&mut cx, &[]);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        matches!(
            cx.state.continuations.top(),
            Some(Continuation::SkillTest(t)) if matches!(t.continuation, SkillTestStep::PreTokenWindow)
        ),
        "the commit hop parks the SkillTest at PreTokenWindow for the loop to drive",
    );
    assert!(
        !cx.events
            .iter()
            .any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "the hop itself does not resolve the test to teardown",
    );

    // The loop's SkillTest arm drives the parked frame the rest of the way.
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        cx.events
            .iter()
            .any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "the loop resolved the test to teardown",
    );
    assert!(
        !cx.state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
        "the SkillTest frame was torn down by the loop",
    );
}
