use super::*;
use crate::test_support::TEST_REGISTRIES;

/// `INV`'s open turn holding `hand`, with a `Numeric(0)` bag.
fn commit_board(hand: &[&str]) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand(hand.iter().copied()))
        .with_chaos_bag(bag_only_zero())
        .open_turn(INV)
}

/// `INV` starts an Intellect 3 test from the turn menu: the session rests at
/// its commit window.
fn paused_at_commit_window(board: GameStateBuilder) -> TestSession {
    board.session().test_skill(INV, SkillKind::Intellect, 3)
}

#[test]
fn perform_skill_test_awaits_input_between_started_and_revealed() {
    // Acceptance: the commit window must open between SkillTestStarted
    // and ChaosTokenRevealed. Starting the test stops at that window with
    // only SkillTestStarted among its events.
    let paused = paused_at_commit_window(commit_board(&[]));

    assert_eq!(paused.prompt().kind, InputKind::PickMultiple);
    assert_event!(
        paused.events(),
        Event::SkillTestStarted { investigator, .. } if *investigator == INV
    );
    // The chaos token has NOT been drawn yet — that fires on the
    // resume path after the commit response arrives.
    assert_no_event!(paused.events(), Event::ChaosTokenRevealed { .. });
    assert!(
        paused.state().has_skill_test_in_flight(),
        "the SkillTest frame must be populated while paused",
    );
}

#[test]
fn resolve_input_with_empty_commits_resumes_the_test() {
    // Pause → resume committing nothing → ChaosTokenRevealed and the rest
    // of resolution fire on the commit step.
    let resumed = paused_at_commit_window(commit_board(&[])).commit(&[]);

    resumed.assert_at_turn_menu(INV);
    let resumed = resumed.finish();
    assert_event!(resumed.events, Event::ChaosTokenRevealed { .. });
    assert_event!(
        resumed.events,
        Event::SkillTestSucceeded { investigator, .. } if *investigator == INV
    );
    assert_event!(
        resumed.events,
        Event::SkillTestEnded { investigator } if *investigator == INV
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
    // fully resolves. The stack is the subject here.
    let at_menu = commit_board(&[]).session();
    let depth_at_menu = at_menu.state().continuations.len();

    let paused = at_menu.test_skill(INV, SkillKind::Intellect, 3);
    assert_eq!(
        paused.state().continuations.len(),
        depth_at_menu + 1,
        "parking at the commit window pushes exactly one frame",
    );
    assert!(
        matches!(
            paused.state().continuations.top(),
            Some(Continuation::SkillTest(_))
        ),
        "the pushed frame is the SkillTest frame carrying the in-flight test",
    );

    let resumed = paused.commit(&[]);
    resumed.assert_at_turn_menu(INV);
    assert_eq!(
        resumed.state().continuations.len(),
        depth_at_menu,
        "resolving the test pops the SkillTest frame",
    );
}

#[test]
fn commit_window_discards_committed_cards_into_discard_pile() {
    // Two cards in hand; commit both. After resolution, both are
    // in the discard pile, neither in hand, and CardDiscarded
    // events fired with `from: Hand`. Icon counting is exercised
    // separately via the cards integration test (this one
    // names game-core's test registry, which knows neither card, so icon
    // contribution is 0).
    let session = paused_at_commit_window(commit_board(&["A", "B"])).commit(&["A", "B"]);

    session.assert_at_turn_menu(INV);
    let result = session.finish();
    let inv_after = &result.state.investigators[&INV];
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
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(investigator) }
            if *investigator == INV && *code == CardCode::new("A")
    );
    assert_event!(
        result.events,
        Event::CardDiscarded { code, from: Zone::Hand, to: DiscardPile::Investigator(investigator) }
            if *investigator == INV && *code == CardCode::new("B")
    );
}

// The next four tests' subject is a malformed or misplaced `ResolveInput`
// itself, so each sends it raw rather than through a step.

#[test]
fn commit_window_rejects_out_of_bounds_index() {
    let paused = paused_at_commit_window(commit_board(&["A"]));
    let bad = paused.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: vec![OptionId(5)],
        },
    }));
    let reason = bad.expect_rejected();
    assert!(
        reason.contains("out of bounds"),
        "unexpected reason: {reason}"
    );
    // State stays paused (engine still in-flight) so a client
    // can submit a fixed-up response without re-initiating.
    assert!(bad.state().has_skill_test_in_flight());
}

#[test]
fn commit_window_rejects_duplicate_indices() {
    let paused = paused_at_commit_window(commit_board(&["A", "B"]));
    let bad = paused.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickMultiple {
            selected: vec![OptionId(0), OptionId(0)],
        },
    }));
    let reason = bad.expect_rejected();
    assert!(reason.contains("duplicate"), "unexpected reason: {reason}");
    // State stays paused so the client can submit a fixed-up
    // response without re-initiating the test.
    assert!(bad.state().has_skill_test_in_flight());
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
    let paused = paused_at_commit_window(commit_board(&[]));
    let bad = paused.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::Confirm,
    }));
    bad.expect_rejected();
    // Test still paused.
    assert!(bad.state().has_skill_test_in_flight());
}

#[test]
fn resolve_input_without_any_outstanding_prompt_rejects() {
    // No prior `apply` opened a commit window — the engine has nothing to
    // resume. The empty stack is the moment under test, so the state is
    // applied to directly rather than settled into a session.
    let state = GameStateBuilder::new().build();
    let result = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
        TEST_REGISTRIES,
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
    let session = take_committing_nothing(
        investigate_board(seat(1).at(HERE), 2, 2),
        &TurnAction::Investigate { investigator: INV },
    );
    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::SkillTestStarted { .. },
        Event::ChaosTokenRevealed { .. },
        Event::SkillTestSucceeded { .. },
        Event::CluePlaced { .. },
        Event::LocationCluesChanged { .. },
        Event::SkillTestEnded { .. },
    );
}
