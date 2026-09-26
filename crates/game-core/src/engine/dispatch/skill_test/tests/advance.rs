use super::*;

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
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let out = perform_skill_test(&mut cx, inv, SkillKind::Intellect, 1);
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    let out = finish_skill_test(&mut cx, &[]);
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert!(state.encounter_discard.is_empty());
}

/// A skill test with an `on_success` effect runs it on a successful
/// draw (the success-side mirror of the `on_fail` path).
#[test]
fn skill_test_runs_on_success_effect_on_a_passing_draw() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    // Willpower 3 + Numeric(0) = 3 vs difficulty 2 → success.
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
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

/// Both ST.1/ST.2 player windows open and auto-skip (no registry / nothing
/// Fast-eligible), bracketing the commit, and the test still resolves. (#374.)
#[test]
fn skill_test_opens_and_auto_skips_both_player_windows() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    // start -> PreCommitWindow auto-skips window 1 -> parks at AwaitingCommit.
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
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "commit prompt (window 1 before commit opened and auto-skipped to it)"
    );

    // commit nothing -> PreTokenWindow auto-skips window 2 -> resolves to end.
    let out = finish_skill_test(&mut cx, &[]);
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(
        out,
        EngineOutcome::Done,
        "window 2 (before token) opened and auto-skipped, then resolved",
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "the test resolved to the end: {events:?}",
    );
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
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
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
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
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
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

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
    // advance parked at AwaitingCommit and emitted the commit prompt.
    let EngineOutcome::AwaitingInput { request, .. } = &out else {
        panic!("expected the commit prompt, got {out:?}");
    };
    assert!(
        request.prompt.contains("Commit cards"),
        "the AwaitingCommit arm emits the commit prompt: {request:?}",
    );
    assert!(matches!(
        cx.state.continuations.last(),
        Some(Continuation::SkillTest(_))
    ));

    // Commit nothing → the hop parks; the loop drives to teardown.
    let out = finish_skill_test(&mut cx, &[]);
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::SkillTestStarted { .. }))
            && events
                .iter()
                .any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "the test ran start-to-end: {events:?}",
    );
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::SkillTest(_))),
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
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    state.interactive_acknowledge = true;
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

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
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "commit prompt"
    );

    // Commit nothing -> resolution runs, then suspends at the acknowledge step.
    let out = finish_skill_test(&mut cx, &[]);
    let out = dispatch::drive(&mut cx, out);
    let EngineOutcome::AwaitingInput { request, .. } = &out else {
        panic!("expected the acknowledge Confirm prompt, got {out:?}");
    };
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
    // Read through `cx.events` here: `cx` is still borrowed mutably for the
    // Confirm resume below, so the outer `events` binding is unavailable.
    assert!(
        cx.events
            .iter()
            .any(|e| matches!(e, Event::ChaosTokenRevealed { .. })),
        "token revealed before the ack"
    );
    assert!(
        cx.events
            .iter()
            .any(|e| matches!(e, Event::SkillTestSucceeded { .. })),
        "outcome logged before the ack"
    );
    assert!(
        !cx.events
            .iter()
            .any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "teardown waits on the acknowledgment"
    );

    // Confirm -> drive into teardown.
    let out = acknowledge_outcome(&mut cx);
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::SkillTestEnded { .. })),
        "after Confirm the test resolved to the end: {events:?}"
    );
}

/// Flag off (default): no acknowledge pause — the test resolves straight
/// through, exactly as before #478 (guards against test churn).
#[test]
fn no_acknowledge_pause_when_flag_off() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

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
    assert!(
        matches!(out, EngineOutcome::AwaitingInput { .. }),
        "commit prompt"
    );
    let out = finish_skill_test(&mut cx, &[]);
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(
        out,
        EngineOutcome::Done,
        "no acknowledge pause when the flag is off"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::SkillTestEnded { .. })));
}

/// `acknowledge_outcome` rejects (state untouched) when there is no in-flight
/// test to acknowledge.
#[test]
fn acknowledge_outcome_rejects_without_in_flight_test() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
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
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(inv)
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(0)];
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

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
            cx.state.continuations.last(),
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
