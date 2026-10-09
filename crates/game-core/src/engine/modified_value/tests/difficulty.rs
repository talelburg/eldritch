use super::*;

/// A card-declared difficulty is a printed base value with nothing
/// behind it on the board.
#[test]
fn a_fixed_difficulty_is_its_printed_number() {
    let state = state_with_test(DifficultyBasis::Fixed(4));
    assert_eq!(difficulty_of(&state), 4);
}

/// An investigation's difficulty *is* the location's modified shroud,
/// attachments and all.
#[test]
fn an_investigations_difficulty_is_the_locations_modified_shroud() {
    let mut state = state_with_test(DifficultyBasis::Shroud(LocationId(3)));
    assert_eq!(difficulty_of(&state), 2, "the printed shroud");
    state
        .locations
        .get_mut(&LocationId(3))
        .unwrap()
        .attachments
        .push(CardInPlay::enter_play(
            CardCode::new("shroud-plus-2"),
            CardInstanceId(0),
            Owner::EncounterDeck,
        ));
    assert_eq!(difficulty_of(&state), 4, "Obscuring Fog's +2, read live");
}

/// A Fight's difficulty is the enemy's modified fight value; an
/// Evade's is its modified evade value.
#[test]
fn an_enemys_fight_and_evade_are_the_difficulties_of_attacking_and_evading_it() {
    let state = state_with_test(DifficultyBasis::Fight(EnemyId(7)));
    assert_eq!(difficulty_of(&state), 2);
    let mut state = state_with_test(DifficultyBasis::Evade(EnemyId(7)));
    assert_eq!(difficulty_of(&state), 2);
    state.enemies.get_mut(&EnemyId(7)).unwrap().evade = 4;
    assert_eq!(difficulty_of(&state), 4, "read live, not banked at ST.1");
}

/// The reduction an initiating effect grants (Flashlight 01087's *"-2
/// shroud for this investigation"*) is a row over the **location**, so
/// it composes with the location's own modifiers and the clamp lands
/// once, at the end: 1 + 2 − 2 = 1, not (1 − 2 → 0) + 2 = 2.
#[test]
fn a_recorded_row_on_a_location_composes_with_its_attachments() {
    let mut state = state_with_test(DifficultyBasis::Shroud(LocationId(3)));
    let loc = state.locations.get_mut(&LocationId(3)).unwrap();
    loc.shroud = 1;
    loc.attachments.push(CardInPlay::enter_play(
        CardCode::new("shroud-plus-2"),
        CardInstanceId(0),
        Owner::EncounterDeck,
    ));
    state.recorded_modifiers.push(RecordedModifier::targeting(
        ModifierTarget::Location(LocationId(3)),
        InvestigatorId(1),
        Stat::Shroud,
        IntExpr::Lit(-2),
        Lifetime::SkillTest(IN_FLIGHT),
        None,
    ));
    assert_eq!(difficulty_of(&state), 1);
}

/// The same reduction with nothing else in play goes below zero and is
/// clamped there — Flashlight's ruling: *"If you reduce shroud to 0,
/// investigating this location will be successful even if you reveal a
/// -8 token"* (<https://arkhamdb.com/card/01087>).
#[test]
fn a_difficulty_reduced_below_zero_clamps_at_zero() {
    let mut state = state_with_test(DifficultyBasis::Shroud(LocationId(3)));
    state.locations.get_mut(&LocationId(3)).unwrap().shroud = 1;
    state.recorded_modifiers.push(RecordedModifier::targeting(
        ModifierTarget::Location(LocationId(3)),
        InvestigatorId(1),
        Stat::Shroud,
        IntExpr::Lit(-2),
        Lifetime::SkillTest(IN_FLIGHT),
        None,
    ));
    assert_eq!(difficulty_of(&state), 0);
}

/// A row bought for another test contributes nothing to this one's
/// difficulty either — the identity check is not investigator-specific.
#[test]
fn a_location_row_from_another_test_does_not_change_the_difficulty() {
    let mut state = state_with_test(DifficultyBasis::Shroud(LocationId(3)));
    state.recorded_modifiers.push(RecordedModifier::targeting(
        ModifierTarget::Location(LocationId(3)),
        InvestigatorId(1),
        Stat::Shroud,
        IntExpr::Lit(-2),
        Lifetime::SkillTest(SkillTestId(999)),
        None,
    ));
    assert_eq!(difficulty_of(&state), 2, "the printed shroud, unreduced");
}

/// Outside a test there is no difficulty to read.
#[test]
fn a_difficulty_with_no_test_in_flight_is_zero() {
    let state = GameStateBuilder::new().build();
    let breakdown = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Test,
        ModifiedQuantity::Difficulty,
        ReadContext::OutsideTest,
    );
    assert_eq!(breakdown.base, 0);
    assert!(breakdown.contributions.is_empty());
}
