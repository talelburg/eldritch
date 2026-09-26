use super::*;

/// Standard-difficulty Night of the Zealot symbol-token values.
fn night_of_the_zealot_standard() -> TokenModifiers {
    TokenModifiers {
        skull: -1,
        cultist: -2,
        tablet: -3,
        elder_thing: -4,
    }
}

#[test]
fn perform_skill_test_with_unknown_investigator_is_rejected() {
    let state = GameStateBuilder::new()
        .with_chaos_bag(bag_only_zero())
        .build();
    let result = test_support::perform_skill_test_no_commits(
        state,
        InvestigatorId(999),
        SkillKind::Willpower,
        0,
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn perform_skill_test_with_empty_bag_is_rejected() {
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 0);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn perform_skill_test_with_negative_difficulty_is_rejected() {
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, -1);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn perform_skill_test_succeeds_when_total_meets_difficulty() {
    // Default skills are 3/3/3/3; bag only has Numeric(0), so total=3.
    // Difficulty 3 → margin 0 → success.
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Intellect, 3);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestStarted { investigator, skill: SkillKind::Intellect, difficulty: 3 }
            if *investigator == id
    );
    assert_event!(
        result.events,
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(0),
            resolution: TokenResolution::Modifier(0),
        }
    );
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    assert_event!(
        result.events,
        Event::SkillTestEnded { investigator } if *investigator == id
    );
    assert_no_event!(result.events, Event::SkillTestFailed { .. });
}

#[test]
fn perform_skill_test_succeeds_with_positive_margin() {
    // Skill 5 + Numeric(0) vs difficulty 2 → margin 3.
    let id = InvestigatorId(1);
    let mut strong = test_support::test_investigator(1);
    strong.skills.combat = 5;
    let state = GameStateBuilder::new()
        .with_investigator(strong)
        .with_chaos_bag(bag_only_zero())
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Combat, 2);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Combat, margin: 3 }
            if *investigator == id
    );
}

#[test]
fn perform_skill_test_fails_when_total_below_difficulty() {
    // Skills 3/3/3/3, bag Numeric(0), difficulty 5 → margin -2 →
    // FailureReason::Total, by: 2.
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Combat, 5);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed {
            investigator,
            skill: SkillKind::Combat,
            reason: FailureReason::Total,
            by: 2,
        } if *investigator == id
    );
    assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
}

#[test]
fn perform_skill_test_autofail_forces_total_to_zero() {
    // Per the Rules Reference: AutoFail makes the investigator's
    // total = 0 (not just "test fails"), so the failure margin is
    // computed against 0. Skill 99 + AutoFail vs difficulty 4 →
    // total 0, by = 4, reason AutoFail.
    let id = InvestigatorId(1);
    let mut high = test_support::test_investigator(1);
    high.skills.willpower = 99;
    let state = GameStateBuilder::new()
        .with_investigator(high)
        .with_chaos_bag(ChaosBag::new([ChaosToken::AutoFail]))
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 4);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed {
            investigator,
            skill: SkillKind::Willpower,
            reason: FailureReason::AutoFail,
            by: 4,
        } if *investigator == id
    );
}

#[test]
fn perform_skill_test_autofail_at_difficulty_zero_still_fails() {
    // Edge case: difficulty 0 would normally succeed at margin 0,
    // but AutoFail forces total = 0 AND tags the result as a
    // failure regardless. by = 0 here, reason = AutoFail.
    // `data/official-faq/Frequently_Asked_Questions.md`: *"No matter
    // what, if you automatically fail a test, you have failed the test,
    // regardless of how your skill value and the difficulty compare."*
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::AutoFail]))
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 0);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 0,
            ..
        }
    );
    assert_no_event!(result.events, Event::SkillTestSucceeded { .. });
}

/// The `[auto_fail]` token latches a determination rather than
/// tripping a special case in the driver (#685), and a determination is
/// a test-scoped recorded row — so it expires with the test that
/// carried it. A leak here would automatically fail the *next* test.
#[test]
fn an_autofail_determination_does_not_outlive_its_test() {
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::AutoFail]))
        .build();
    let first = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 2);
    assert!(
        first.state.recorded_modifiers.is_empty(),
        "the determination expires with the test's teardown",
    );

    // The bag still holds only the AutoFail, so the second test fails
    // automatically too — but on a determination latched afresh, which
    // is what makes `by: 2` (skill value 0 against difficulty 2) rather
    // than the artefact of a stale row a leak would produce.
    let second =
        test_support::perform_skill_test_no_commits(first.state, id, SkillKind::Willpower, 2);
    assert_event!(
        second.events,
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 2,
            ..
        }
    );
}

#[test]
fn perform_skill_test_clamps_negative_total_to_zero() {
    // skill 3 + Skull(−6) = −3, clamped to 0. Difficulty 2 →
    // by = 2, reason Total (NOT AutoFail).
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Skull]))
        .with_token_modifiers(TokenModifiers {
            skull: -6,
            ..TokenModifiers::default()
        })
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 2);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::SkillTestFailed {
            reason: FailureReason::Total,
            by: 2,
            ..
        }
    );
}

#[test]
fn perform_skill_test_elder_sign_treated_as_modifier_zero() {
    // ElderSign as +0 placeholder until per-investigator ability
    // dispatch lands. Skill 3, difficulty 3 → margin 0 → success.
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Agility, 3);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::ChaosTokenRevealed {
            token: ChaosToken::ElderSign,
            resolution: TokenResolution::ElderSign,
        }
    );
    assert_event!(result.events, Event::SkillTestSucceeded { margin: 0, .. });
}

#[test]
fn perform_skill_test_symbol_token_modifier_applies() {
    // Bag is one Skull. Standard-difficulty NotZ: skull = -1.
    // Skill 3 + (-1) = 2 vs difficulty 2 → margin 0 → success.
    let id = InvestigatorId(1);
    let state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Skull]))
        .with_token_modifiers(night_of_the_zealot_standard())
        .build();
    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 2);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::Modifier(-1),
        }
    );
    assert_event!(result.events, Event::SkillTestSucceeded { margin: 0, .. });
}

#[test]
fn a_skill_tests_teardown_expires_only_its_own_recorded_rows() {
    // A recorded row belongs to the test it names (#676). The test run
    // here mints its own id, so a row stamped with a different one is
    // neither counted by it (skill 3 vs difficulty 4 fails by 1, despite
    // the +1 row) nor swept away by its teardown.
    let id = InvestigatorId(1);
    let other_test = SkillTestId(99);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(bag_only_zero())
        .build();
    state.recorded_modifiers = vec![RecordedModifier::new(
        id,
        Stat::Willpower,
        IntExpr::Lit(1),
        Lifetime::SkillTest(other_test),
        None,
    )];

    let result = test_support::perform_skill_test_no_commits(state, id, SkillKind::Willpower, 4);
    assert_eq!(result.outcome, EngineOutcome::Done);
    // A row belonging to another test contributes nothing: skill 3
    // against difficulty 4 fails by 1, +1 row or no.
    assert_event!(result.events, Event::SkillTestFailed { by: 1, .. });
    assert_eq!(
        result.state.recorded_modifiers.len(),
        1,
        "and is not expired by this test's teardown either",
    );
    assert_eq!(
        result.state.recorded_modifiers[0].lifetime,
        Lifetime::SkillTest(other_test),
    );
}

#[test]
fn perform_skill_test_advances_rng_and_log_round_trips() {
    // Determinism: starting the same plain skill test twice
    // from identical initial state produces identical post-state.
    let id = InvestigatorId(1);
    let initial = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_chaos_bag(ChaosBag::new([
            ChaosToken::Numeric(1),
            ChaosToken::Numeric(-1),
            ChaosToken::Skull,
        ]))
        .with_token_modifiers(night_of_the_zealot_standard())
        .with_rng_seed(123)
        .build();
    let first =
        test_support::perform_skill_test_no_commits(initial.clone(), id, SkillKind::Willpower, 3);
    let second = test_support::perform_skill_test_no_commits(initial, id, SkillKind::Willpower, 3);

    assert_eq!(first.outcome, EngineOutcome::Done);
    assert_eq!(first.state.rng, second.state.rng);
    assert_eq!(first.state.rng.draws, 1);
    assert_eq!(first.events, second.events);
}
