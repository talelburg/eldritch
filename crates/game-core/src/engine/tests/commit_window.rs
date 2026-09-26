use super::*;

#[test]
fn perform_skill_test_awaits_input_between_started_and_revealed() {
    // Acceptance: AwaitingInput must fire between SkillTestStarted
    // and ChaosTokenRevealed. The first `apply` returns
    // AwaitingInput with only SkillTestStarted on the events list.
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    let result = test_support::perform_skill_test(state, id, SkillKind::Intellect, 3);

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::SkillTestStarted { investigator, .. } if *investigator == id
    );
    // The chaos token has NOT been drawn yet — that fires on the
    // resume path after the commit response arrives.
    assert_no_event!(result.events, Event::ChaosTokenRevealed { .. });
    assert!(
        result.state.has_skill_test_in_flight(),
        "the SkillTest frame must be populated while paused",
    );
}

#[test]
fn resolve_input_with_empty_commits_resumes_the_test() {
    // Pause → resume with `PickMultiple { selected: [] }` →
    // ChaosTokenRevealed and the rest of resolution fire on the
    // second apply.
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    let paused = test_support::perform_skill_test(state, id, SkillKind::Intellect, 3);

    let resumed = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );

    assert_eq!(resumed.outcome, EngineOutcome::Done);
    assert_event!(resumed.events, Event::ChaosTokenRevealed { .. });
    assert_event!(
        resumed.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == id
    );
    assert_event!(
        resumed.events,
        Event::SkillTestEnded { investigator } if *investigator == id
    );
    assert!(
        !resumed.state.has_skill_test_in_flight(),
        "the SkillTest frame must clear after resolution",
    );
}

#[test]
fn skill_test_pushes_and_pops_a_continuation_frame() {
    // Axis-B T4: the commit window is a `Continuation::SkillTest` frame
    // on the one stack, pushed when the test parks and popped when it
    // fully resolves.
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    let paused = test_support::perform_skill_test(state, id, SkillKind::Intellect, 3);
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_eq!(
        paused.state.continuations.len(),
        1,
        "parking at the commit window pushes exactly one frame",
    );
    assert!(
        matches!(
            paused.state.continuations.first(),
            Some(Continuation::SkillTest(_))
        ),
        "the single frame is the SkillTest frame carrying the in-flight test",
    );

    let resumed = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert_eq!(resumed.outcome, EngineOutcome::Done);
    assert!(
        resumed.state.continuations.is_empty(),
        "resolving the test pops the SkillTest frame",
    );
}

#[test]
fn commit_window_discards_committed_cards_into_discard_pile() {
    // Two cards in hand; commit both. After resolution, both are
    // in the discard pile, neither in hand, and CardDiscarded
    // events fired with `from: Hand`. Icon counting is exercised
    // separately via the cards integration test (this one
    // doesn't install a registry, so icon contribution is 0).
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new("A"), CardCode::new("B")];
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_chaos_bag(bag_only_zero())
        .build();

    let result = perform_skill_test_with_response(
        state,
        id,
        SkillKind::Intellect,
        3,
        InputResponse::PickMultiple {
            selected: vec![OptionId(0), OptionId(1)],
        },
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv_after = &result.state.investigators[&id];
    assert!(
        inv_after.hand.is_empty(),
        "hand must be empty after commit + discard"
    );
    assert_eq!(
        inv_after.discard,
        vec![CardCode::new("A"), CardCode::new("B")],
        "committed cards land in discard in commit order",
    );
    assert_event_count!(result.events, 2, Event::CardDiscarded { .. });
    assert_event!(
        result.events,
        Event::CardDiscarded { investigator, code, from: Zone::Hand }
            if *investigator == id && *code == CardCode::new("A")
    );
    assert_event!(
        result.events,
        Event::CardDiscarded { investigator, code, from: Zone::Hand }
            if *investigator == id && *code == CardCode::new("B")
    );
}

/// Helper: start a plain skill test and drive it through with the
/// given `InputResponse`. Used by commit-window tests that don't fit
/// `perform_skill_test_no_commits` (which always submits an empty commit).
fn perform_skill_test_with_response(
    state: GameState,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
    response: InputResponse,
) -> ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.push(response);
    test_support::drive_skill_test(state, investigator, skill, difficulty, resolver)
}

#[test]
fn commit_window_rejects_out_of_bounds_index() {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new("A")];
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_chaos_bag(bag_only_zero())
        .build();
    let paused = test_support::perform_skill_test(state, id, SkillKind::Intellect, 3);
    let bad = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: vec![OptionId(5)],
            },
        }),
    );
    match bad.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(
                reason.contains("out of bounds"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    // State stays paused (engine still in-flight) so a client
    // can submit a fixed-up response without re-initiating.
    assert!(bad.state.has_skill_test_in_flight());
}

#[test]
fn commit_window_rejects_duplicate_indices() {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new("A"), CardCode::new("B")];
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_chaos_bag(bag_only_zero())
        .build();
    let paused = test_support::perform_skill_test(state, id, SkillKind::Intellect, 3);
    let bad = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: vec![OptionId(0), OptionId(0)],
            },
        }),
    );
    match bad.outcome {
        EngineOutcome::Rejected { reason } => {
            assert!(reason.contains("duplicate"), "unexpected reason: {reason}");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    // State stays paused so the client can submit a fixed-up
    // response without re-initiating the test.
    assert!(bad.state.has_skill_test_in_flight());
}

// NOTE: `non_resolve_input_action_rejects_while_skill_test_paused` was
// removed here: it used a variant as a proxy for "any non-ResolveInput action"
// to exercise the pending-prompt gate in `apply_player_action`. Once
// `PlayerAction` collapsed to a single `ResolveInput` variant (#459), the
// gate's `!matches!(action, ResolveInput)` condition became dead and the gate
// itself was removed — there is no non-`ResolveInput` action left to proxy
// with. Pending-prompt protection is now structural: `resolve_input` rejects a
// `ResolveInput` that arrives with no outstanding prompt, and the
// wrong-response-kind rejection is pinned in
// `resolve_input_with_wrong_response_variant_rejects`.

#[test]
fn resolve_input_with_wrong_response_variant_rejects() {
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    let paused = test_support::perform_skill_test(state, id, SkillKind::Intellect, 3);
    let bad = apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Confirm,
        }),
    );
    assert!(matches!(bad.outcome, EngineOutcome::Rejected { .. }));
    // Test still paused.
    assert!(bad.state.has_skill_test_in_flight());
}

#[test]
fn resolve_input_without_any_outstanding_prompt_rejects() {
    // No prior `apply` opened a commit window — the engine has
    // nothing to resume.
    let state = GameStateBuilder::new().build();
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
}

#[test]
fn investigate_canonical_event_sequence_pins_followup_before_test_ended() {
    // Pins the post-#63 ordering: on a successful Investigate the
    // discover-clue events fire *before* SkillTestEnded, matching
    // the `SkillTestEnded` event-doc text that cleanup precedes
    // the end marker. The pre-#63 ordering ran the follow-up
    // after the bracketing end event — silently flipping that
    // back would break downstream listeners that key off the end
    // marker as "all sub-effects already applied."
    let (inv_id, _loc_id, state) = investigate_scenario(2, 2);
    let result = take_action_no_commits(
        state,
        &TurnAction::Investigate {
            investigator: inv_id,
        },
    );
    assert!(!matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_event_sequence!(
        result.events,
        Event::SkillTestStarted { .. },
        Event::ChaosTokenRevealed { .. },
        Event::SkillTestSucceeded { .. },
        Event::CluePlaced { .. },
        Event::LocationCluesChanged { .. },
        Event::SkillTestEnded { .. },
    );
}
