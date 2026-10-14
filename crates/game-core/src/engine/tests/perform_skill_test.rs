use super::*;
use crate::test_support::TEST_REGISTRIES;

/// Standard-difficulty Night of the Zealot symbol-token values.
fn night_of_the_zealot_standard() -> TokenModifiers {
    TokenModifiers {
        skull: -1,
        cultist: -2,
        tablet: -3,
        elder_thing: -4,
    }
}

/// `tester`'s open turn, with `bag` as the chaos bag.
fn skill_test_board(tester: Seat, bag: ChaosBag) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(tester)
        .with_chaos_bag(bag)
        .open_turn(INV)
}

/// From the turn menu, `INV` tests `skill` against `difficulty` committing
/// nothing: the session comes back to the menu.
fn run_test(session: TestSession, skill: SkillKind, difficulty: i8) -> TestSession {
    let session = session.test_skill(INV, skill, difficulty).commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// `INV`'s test of `skill` against `difficulty` on `board` was rejected as it
/// started: no events, and the session is still at the turn menu.
fn assert_rejected_at_start(
    board: GameStateBuilder,
    investigator: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
) {
    let session = board.session().test_skill(investigator, skill, difficulty);
    session.expect_rejected();
    session.assert_at_turn_menu(INV);
    assert!(session.finish().events.is_empty());
}

#[test]
fn perform_skill_test_with_unknown_investigator_is_rejected() {
    assert_rejected_at_start(
        skill_test_board(seat(1), bag_only_zero()),
        InvestigatorId(999),
        SkillKind::Willpower,
        0,
    );
}

#[test]
fn perform_skill_test_with_empty_bag_is_rejected() {
    let board = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .open_turn(INV);
    assert_rejected_at_start(board, INV, SkillKind::Willpower, 0);
}

#[test]
fn perform_skill_test_with_negative_difficulty_is_rejected() {
    assert_rejected_at_start(
        skill_test_board(seat(1), bag_only_zero()),
        INV,
        SkillKind::Willpower,
        -1,
    );
}

#[test]
fn perform_skill_test_succeeds_when_total_meets_difficulty() {
    // Default skills are 3/3/3/3; bag only has Numeric(0), so total=3.
    // Difficulty 3 → margin 0 → success.
    let session = run_test(
        skill_test_board(seat(1), bag_only_zero()).session(),
        SkillKind::Intellect,
        3,
    );

    let events = session.events();
    assert_event!(
        events,
        Event::SkillTestStarted { investigator, skill: SkillKind::Intellect, difficulty: 3 }
            if *investigator == INV
    );
    assert_event!(
        events,
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(0),
            resolution: TokenResolution::Modifier(0),
        }
    );
    assert_event!(
        events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    assert_event!(
        events,
        Event::SkillTestEnded { investigator } if *investigator == INV
    );
    assert_no_event!(events, Event::SkillTestFailed { .. });
}

#[test]
fn perform_skill_test_succeeds_with_positive_margin() {
    // Skill 5 + Numeric(0) vs difficulty 2 → margin 3.
    let strong = seat(1).skill(SkillKind::Combat, 5);
    let session = run_test(
        skill_test_board(strong, bag_only_zero()).session(),
        SkillKind::Combat,
        2,
    );

    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Combat, margin: 3 }
            if *investigator == INV
    );
}

#[test]
fn perform_skill_test_fails_when_total_below_difficulty() {
    // Skills 3/3/3/3, bag Numeric(0), difficulty 5 → margin -2 →
    // FailureReason::Total, by: 2.
    let session = run_test(
        skill_test_board(seat(1), bag_only_zero()).session(),
        SkillKind::Combat,
        5,
    );

    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            investigator,
            skill: SkillKind::Combat,
            reason: FailureReason::Total,
            by: 2,
        } if *investigator == INV
    );
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
}

#[test]
fn perform_skill_test_autofail_forces_total_to_zero() {
    // Per the Rules Reference: AutoFail makes the investigator's
    // total = 0 (not just "test fails"), so the failure margin is
    // computed against 0. Skill 99 + AutoFail vs difficulty 4 →
    // total 0, by = 4, reason AutoFail.
    let high = seat(1).skill(SkillKind::Willpower, 99);
    let session = run_test(
        skill_test_board(high, ChaosBag::new([ChaosToken::AutoFail])).session(),
        SkillKind::Willpower,
        4,
    );

    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            investigator,
            skill: SkillKind::Willpower,
            reason: FailureReason::AutoFail,
            by: 4,
        } if *investigator == INV
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
    let session = run_test(
        skill_test_board(seat(1), ChaosBag::new([ChaosToken::AutoFail])).session(),
        SkillKind::Willpower,
        0,
    );

    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            reason: FailureReason::AutoFail,
            by: 0,
            ..
        }
    );
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
}

/// The `[auto_fail]` token latches a determination rather than
/// tripping a special case in the driver (#685), and a determination is
/// a test-scoped recorded row — so it expires with the test that
/// carried it. A leak here would automatically fail the *next* test.
#[test]
fn an_autofail_determination_does_not_outlive_its_test() {
    let first = run_test(
        skill_test_board(seat(1), ChaosBag::new([ChaosToken::AutoFail])).session(),
        SkillKind::Willpower,
        2,
    );
    assert!(
        first.state().recorded_modifiers.is_empty(),
        "the determination expires with the test's teardown",
    );

    // The bag still holds only the AutoFail, so the second test fails
    // automatically too — but on a determination latched afresh, which
    // is what makes `by: 2` (skill value 0 against difficulty 2) rather
    // than the artefact of a stale row a leak would produce.
    let second = run_test(first, SkillKind::Willpower, 2).finish();
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
    let board = skill_test_board(seat(1), ChaosBag::new([ChaosToken::Skull])).with_token_modifiers(
        TokenModifiers {
            skull: -6,
            ..TokenModifiers::default()
        },
    );
    let session = run_test(board.session(), SkillKind::Willpower, 2);

    assert_event!(
        session.events(),
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
    let session = run_test(
        skill_test_board(seat(1), ChaosBag::new([ChaosToken::ElderSign])).session(),
        SkillKind::Agility,
        3,
    );

    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::ElderSign,
            resolution: TokenResolution::ElderSign,
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 0, .. }
    );
}

#[test]
fn perform_skill_test_symbol_token_modifier_applies() {
    // Bag is one Skull. Standard-difficulty NotZ: skull = -1.
    // Skill 3 + (-1) = 2 vs difficulty 2 → margin 0 → success.
    let board = skill_test_board(seat(1), ChaosBag::new([ChaosToken::Skull]))
        .with_token_modifiers(night_of_the_zealot_standard());
    let session = run_test(board.session(), SkillKind::Willpower, 2);

    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::Modifier(-1),
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 0, .. }
    );
}

#[test]
fn a_skill_tests_teardown_expires_only_its_own_recorded_rows() {
    // A recorded row belongs to the test it names (#676). The test run
    // here mints its own id, so a row stamped with a different one is
    // neither counted by it (skill 3 vs difficulty 4 fails by 1, despite
    // the +1 row) nor swept away by its teardown.
    let other_test = SkillTestId(99);
    let mut state = skill_test_board(seat(1), bag_only_zero()).build();
    // No builder setter for recorded rows.
    state.recorded_modifiers = vec![RecordedModifier::new(
        INV,
        Stat::Willpower,
        IntExpr::Lit(1),
        Lifetime::SkillTest(other_test),
        None,
    )];

    let session = run_test(
        TestSession::new(state, TEST_REGISTRIES),
        SkillKind::Willpower,
        4,
    );
    // A row belonging to another test contributes nothing: skill 3
    // against difficulty 4 fails by 1, +1 row or no.
    assert_event!(session.events(), Event::SkillTestFailed { by: 1, .. });
    let state = session.state();
    assert_eq!(
        state.recorded_modifiers.len(),
        1,
        "and is not expired by this test's teardown either",
    );
    assert_eq!(
        state.recorded_modifiers[0].lifetime,
        Lifetime::SkillTest(other_test),
    );
}

#[test]
fn perform_skill_test_advances_rng_and_log_round_trips() {
    // Determinism: starting the same plain skill test twice
    // from identical initial state produces identical post-state.
    let initial = skill_test_board(
        seat(1),
        ChaosBag::new([
            ChaosToken::Numeric(1),
            ChaosToken::Numeric(-1),
            ChaosToken::Skull,
        ]),
    )
    .with_token_modifiers(night_of_the_zealot_standard())
    .with_rng_seed(123)
    .build();
    let first = run_test(
        TestSession::new(initial.clone(), TEST_REGISTRIES),
        SkillKind::Willpower,
        3,
    );
    let second = run_test(
        TestSession::new(initial, TEST_REGISTRIES),
        SkillKind::Willpower,
        3,
    );

    assert_eq!(first.state().rng, second.state().rng);
    assert_eq!(first.state().rng.draws, 1);
    assert_eq!(first.events(), second.events());
}
