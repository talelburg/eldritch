use super::*;

/// A determination row scoped to [`IN_FLIGHT`].
fn determination_row(d: Determination) -> RecordedModifier {
    RecordedModifier::determination(InvestigatorId(1), d, Lifetime::SkillTest(IN_FLIGHT), None)
}

/// The board [`state_with_test`] builds (investigator 1 taking an
/// Intellect investigation against the Study's shroud 2), with `rows`
/// recorded on top.
fn state_with_determinations(rows: Vec<RecordedModifier>) -> GameState {
    let mut state = state_with_test(DifficultyBasis::Shroud(LocationId(3)));
    state.recorded_modifiers = rows;
    state
}

/// `glossary/Automatic_Failure_Success.md`: *"If a skill test
/// automatically fails, the investigator's total skill value for that
/// test is considered 0."* The difficulty is **not** touched, which is
/// what keeps the margin real — 0 against shroud 2 fails by 2.
#[test]
fn an_automatic_failure_substitutes_the_testers_total_skill_value() {
    let state = state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
    assert_eq!(skill_of(&state, SkillKind::Intellect), 0);
    assert_eq!(difficulty_of(&state), 2, "the difficulty is left alone");
}

/// *"the investigator's total modified skill value is still
/// determined, as it may have some bearing on other card abilities"* —
/// so the substitution replaces the **total**, and the breakdown that
/// produced it survives intact underneath.
#[test]
fn an_automatic_failure_keeps_the_breakdown_it_substitutes() {
    let mut state =
        state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
    state
        .recorded_modifiers
        .push(recorded(InvestigatorId(1), Stat::Intellect, 2));
    let breakdown = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(InvestigatorId(1)),
        ModifiedQuantity::Skill(SkillKind::Intellect),
        ReadContext::DuringTest(SkillTestKind::Investigate),
    );
    assert_eq!(breakdown.base, 3, "the printed intellect");
    assert_eq!(breakdown.contributions.len(), 1, "the +2 row still counts");
    assert_eq!(breakdown.substitution, Some(0));
    assert_eq!(breakdown.total(), 0);
}

/// *"If a skill test automatically succeeds, the total difficulty of
/// that test is considered 0."* The location's own shroud is
/// unchanged: the substitution lands on the **test's** difficulty, not
/// on the quantity it is read from.
#[test]
fn an_automatic_success_substitutes_the_tests_total_difficulty() {
    let state = state_with_determinations(vec![determination_row(Determination::AutomaticSuccess)]);
    assert_eq!(difficulty_of(&state), 0);
    assert_eq!(
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Location(LocationId(3)),
            ModifiedQuantity::Shroud,
            ReadContext::DuringTest(SkillTestKind::Investigate),
        )
        .total(),
        2,
        "the location's shroud is still 2",
    );
    assert_eq!(
        skill_of(&state, SkillKind::Intellect),
        3,
        "the skill value is left alone",
    );
}

/// Automatic failure beats automatic success (ADR 0007), and the two
/// rows coexist: neither suppresses the other, so the answer cannot
/// depend on which was latched first. Asserted in both orders — the
/// bug this rules out is a 0-versus-0 comparison passing as a success.
#[test]
fn automatic_failure_beats_automatic_success_in_either_latch_order() {
    for rows in [
        vec![
            determination_row(Determination::AutomaticFailure),
            determination_row(Determination::AutomaticSuccess),
        ],
        vec![
            determination_row(Determination::AutomaticSuccess),
            determination_row(Determination::AutomaticFailure),
        ],
    ] {
        let state = state_with_determinations(rows);
        assert_eq!(
            test_determination(&state, ReadContext::DuringTest(SkillTestKind::Investigate)),
            Some(Determination::AutomaticFailure),
        );
        assert_eq!(skill_of(&state, SkillKind::Intellect), 0);
        assert_eq!(
            difficulty_of(&state),
            2,
            "the losing automatic success must not zero the difficulty \
             too, or 0 versus 0 compares as a success",
        );
    }
}

/// *"the investigator's total skill value **for that test**"* — the
/// three skills the test is not against are untouched, and so is
/// anyone else's.
#[test]
fn an_automatic_failure_zeroes_only_the_tested_skill() {
    let state = state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
    assert_eq!(
        skill_of(&state, SkillKind::Intellect),
        0,
        "the tested skill"
    );
    assert_eq!(skill_of(&state, SkillKind::Willpower), 3);
    assert_eq!(skill_of(&state, SkillKind::Combat), 3);
    assert_eq!(skill_of(&state, SkillKind::Agility), 3);
}

/// A determination is a recorded row like any other, so it carries the
/// skill-test identity check: one bought for another test is inert.
#[test]
fn a_determination_scoped_to_another_test_is_inert() {
    let mut state = state_with_determinations(vec![RecordedModifier::determination(
        InvestigatorId(1),
        Determination::AutomaticFailure,
        Lifetime::SkillTest(SkillTestId(99)),
        None,
    )]);
    assert_eq!(skill_of(&state, SkillKind::Intellect), 3);
    assert_eq!(
        test_determination(&state, ReadContext::DuringTest(SkillTestKind::Investigate)),
        None,
    );
    // And it is gone for good once its own test tears down.
    state.expire_modifiers_for_test(SkillTestId(99));
    assert!(state.recorded_modifiers.is_empty());
}

/// A read that declares itself outside a test (prey ranking) sees no
/// determination, exactly as it sees no recorded row.
#[test]
fn a_read_outside_a_test_sees_no_determination() {
    let state = state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
    assert_eq!(test_determination(&state, ReadContext::OutsideTest), None,);
    assert_eq!(
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(InvestigatorId(1)),
            ModifiedQuantity::Skill(SkillKind::Intellect),
            ReadContext::OutsideTest,
        )
        .total(),
        3,
    );
}
