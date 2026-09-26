use super::*;

/// One investigator, `rows` recorded, and the test identified by
/// [`IN_FLIGHT`] in flight — the shape a `ThisSkillTest` row is read
/// under.
fn state_with_recorded(rows: Vec<RecordedModifier>) -> GameState {
    state_with_recorded_during(rows, IN_FLIGHT)
}

/// As [`state_with_recorded`], with the in-flight test's id chosen by
/// the caller — so a row can be read against a *different* test.
fn state_with_recorded_during(rows: Vec<RecordedModifier>, in_flight: SkillTestId) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state.recorded_modifiers = rows;
    state
        .continuations
        .push(Continuation::SkillTest(test_support::test_skill_test(
            in_flight,
            InvestigatorId(1),
            SkillKind::Willpower,
            SkillTestKind::Plain,
            2,
        )));
    state
}

#[test]
fn recorded_rows_for_the_target_are_summed() {
    let id = InvestigatorId(1);
    let state = state_with_recorded(vec![
        recorded(id, Stat::Intellect, 1),
        recorded(id, Stat::Intellect, 2),
    ]);
    assert_eq!(skill(&state, id, SkillKind::Intellect), 6);
}

#[test]
fn a_recorded_row_for_another_investigator_is_ignored() {
    let state = state_with_recorded(vec![recorded(InvestigatorId(2), Stat::Willpower, 5)]);
    assert_eq!(skill(&state, InvestigatorId(1), SkillKind::Willpower), 3);
}

#[test]
fn a_recorded_row_for_another_stat_is_ignored() {
    let id = InvestigatorId(1);
    let state = state_with_recorded(vec![
        recorded(id, Stat::Intellect, 1),
        recorded(id, Stat::MaxHealth, 1),
    ]);
    assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
}

/// The identity check, and the reason it exists: a row bought for one
/// test contributes nothing to another. Not merely "it was drained in
/// time" — a row that somehow survived its test is **inert**.
#[test]
fn a_recorded_row_from_another_test_contributes_nothing() {
    let id = InvestigatorId(1);
    let state = state_with_recorded_during(
        vec![recorded_for(id, Stat::Willpower, 5, SkillTestId(1))],
        SkillTestId(2),
    );
    assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
}

/// A `ThisSkillTest` row is scoped to a test, so a read that declares
/// itself outside one — prey ranking — must not see it, even with
/// that very test in flight.
#[test]
fn a_recorded_row_is_invisible_to_an_outside_test_read() {
    let id = InvestigatorId(1);
    let state = state_with_recorded(vec![recorded(id, Stat::Willpower, 5)]);
    assert_eq!(
        modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(id),
            ModifiedQuantity::Skill(SkillKind::Willpower),
            ReadContext::OutsideTest,
        )
        .total(),
        3,
    );
}

/// With no test in flight there is no id to match, so a stray row
/// cannot contribute however the read describes itself.
#[test]
fn a_recorded_row_contributes_nothing_with_no_test_in_flight() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state.recorded_modifiers = vec![recorded(id, Stat::Willpower, 5)];
    assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
}

/// A row's delta is an expression resolved at read time, not a number
/// frozen when the row was pushed: a `Count` row answers from the
/// board as it stands at the read.
#[test]
fn a_recorded_rows_delta_is_evaluated_at_read_time() {
    let id = InvestigatorId(1);
    let loc = LocationId(3);
    let mut state = state_with_recorded(vec![RecordedModifier::new(
        id,
        Stat::Willpower,
        IntExpr::Count(Quantity::CluesAtControllerLocation),
        Lifetime::SkillTest(IN_FLIGHT),
        None,
    )]);
    state
        .locations
        .insert(loc, test_support::test_location(3, "Study"));
    state.investigators.get_mut(&id).unwrap().current_location = Some(loc);
    state.locations.get_mut(&loc).unwrap().clues = 2;
    assert_eq!(skill(&state, id, SkillKind::Willpower), 5);
    // Same row, different board: the answer moves with it, with no
    // invalidation step in between.
    state.locations.get_mut(&loc).unwrap().clues = 4;
    assert_eq!(skill(&state, id, SkillKind::Willpower), 7);
}
