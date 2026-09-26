use super::*;

/// The investigator card lives in `investigator_card`, not in
/// `cards_in_play`; the sweep walks `controlled_card_instances()`,
/// which yields it first.
#[test]
fn a_seated_investigator_cards_modifier_is_counted() {
    let (mut state, id) = state_with_cards_in_play(&[]);
    state
        .investigators
        .get_mut(&id)
        .unwrap()
        .investigator_card
        .code = CardCode::new("inv-willpower-plus-2");
    assert!(
        state.investigators[&id].cards_in_play.is_empty(),
        "the modifier must come from the investigator card, not cards_in_play"
    );
    assert_eq!(skill(&state, id, SkillKind::Willpower), 5);
}

#[test]
fn matching_contributions_are_summed() {
    let (state, id) =
        state_with_cards_in_play(&["willpower-plus-1", "willpower-plus-1", "willpower-plus-1"]);
    assert_eq!(skill(&state, id, SkillKind::Willpower), 6);
}

#[test]
fn a_modifier_to_another_skill_does_not_contribute() {
    let (state, id) = state_with_cards_in_play(&["intellect-plus-2"]);
    assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
    assert_eq!(skill(&state, id, SkillKind::Intellect), 5);
}

/// `ThisSkillTest` is a recorded scope: a card declaring it under
/// `Trigger::Constant` is not swept.
#[test]
fn a_non_constant_scope_is_not_swept() {
    let (state, id) = state_with_cards_in_play(&["willpower-plus-1-this-test-only"]);
    assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
}

/// An `OnPlay` Modify resolved once when the card was played; it is
/// not a standing contribution.
#[test]
fn a_non_constant_trigger_is_not_swept() {
    let (state, id) = state_with_cards_in_play(&["non-constant-willpower"]);
    assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
}

#[test]
fn a_capacity_modifier_never_lands_on_a_skill() {
    let (state, id) = state_with_cards_in_play(&["max-health-plus-1"]);
    for kind in [
        SkillKind::Willpower,
        SkillKind::Intellect,
        SkillKind::Combat,
        SkillKind::Agility,
    ] {
        assert_eq!(skill(&state, id, kind), 3, "{kind:?}");
    }
}

/// Max health reads the printed capacity off the installed registry
/// and folds the same sweep over it.
#[test]
fn a_capacity_modifier_lands_on_max_health() {
    test_support::install_test_registry();
    let (state, id) = state_with_cards_in_play(&["max-health-plus-1", "willpower-plus-1"]);
    let health = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(id),
        ModifiedQuantity::MaxHealth,
        ReadContext::OutsideTest,
    );
    assert_eq!(health.base, 8, "TEST_INV's printed health");
    assert_eq!(health.total(), 9, "the willpower buff must not leak in");
}

/// A card in play whose code the registry can't resolve is skipped
/// silently — the deck-import gate keeps unimplemented codes out of
/// play, so an unresolvable code is engine-only test data.
#[test]
fn an_unknown_code_is_skipped() {
    let (state, id) = state_with_cards_in_play(&["willpower-plus-1", "unknown-card"]);
    assert_eq!(skill(&state, id, SkillKind::Willpower), 4);
}

#[test]
fn an_unknown_investigator_has_no_value_at_all() {
    let state = GameStateBuilder::new().build();
    let breakdown = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(InvestigatorId(99)),
        ModifiedQuantity::Skill(SkillKind::Willpower),
        ReadContext::DuringTest(SkillTestKind::Plain),
    );
    assert_eq!(breakdown.base, 0);
    assert!(breakdown.contributions.is_empty());
}

#[test]
fn with_no_registry_only_the_base_answers() {
    let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
    let breakdown = modified_value(
        &state,
        None,
        ModifierTarget::Investigator(id),
        ModifiedQuantity::Skill(SkillKind::Willpower),
        ReadContext::DuringTest(SkillTestKind::Plain),
    );
    assert_eq!(breakdown.base, 3);
    assert!(breakdown.contributions.is_empty());
}

/// The board both granted-modifier tests read: one location, one
/// investigator standing in it, and the `self-granting-combat` card either
/// under that investigator's control or in play *at* the location under
/// nobody's.
fn granting_card_board(controlled: bool) -> (GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let loc = LocationId(3);
    let card = CardInPlay::enter_play(CardCode::new("self-granting-combat"), CardInstanceId(0));
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc);
    let mut location = test_support::test_location(3, "Study");
    if controlled {
        inv.cards_in_play.push(card);
    } else {
        location.cards_at_location.push(card);
    }
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(location)
        .build();
    (state, id)
}

/// **ADR 0014's claim, for modifiers.** A granted ability is an ability, so
/// a bare `Effect::Modify` a card is *granted* must reach the sweep exactly
/// as a printed one does. Before #773 the two sweeps were parallel and
/// uncomposed: `abilities_in_effect::for_source` merged printed + granted
/// for every ability path, and this one read `abilities_for` — so the
/// granted modifier was found by nothing.
#[test]
fn a_granted_modifier_is_swept() {
    let (state, id) = granting_card_board(true);
    let breakdown = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(id),
        ModifiedQuantity::Skill(SkillKind::Combat),
        ReadContext::DuringTest(SkillTestKind::Plain),
    );
    assert_eq!(breakdown.base, 3, "test_investigator's printed combat");
    assert_eq!(
        breakdown.contributions,
        vec![Contribution {
            source: ContributionSource::Card {
                code: CardCode::new("self-granting-combat"),
                instance: Some(CardInstanceId(0)),
            },
            delta: 1,
        }],
        "the contribution is attributed to the card that has the ability, \
         which is the recipient — the granter is named by the address, and \
         a modifier read carries no address",
    );
    assert_eq!(breakdown.total(), 4);
}

/// The other half: nothing is remembered, so flipping the grant's condition
/// removes the modifier with no invalidation step. Here the flip is the
/// card moving out of a player's control and into the location — the Parlor
/// 01115 / Lita Chantler 01117 transition, in the direction that turns her
/// buffs off.
#[test]
fn a_granted_modifier_vanishes_when_the_grants_condition_flips() {
    let (state, id) = granting_card_board(false);
    let breakdown = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(id),
        ModifiedQuantity::Skill(SkillKind::Combat),
        ReadContext::DuringTest(SkillTestKind::Plain),
    );
    assert!(
        breakdown.contributions.is_empty(),
        "the card is at the investigator's location and its audience is \
         location-scoped, so only the failed `ByAPlayer` condition can be \
         keeping the modifier out",
    );
    assert_eq!(breakdown.total(), 3);
}
