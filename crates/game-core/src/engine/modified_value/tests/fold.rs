use super::*;

/// `Modifiers.md`'s own worked example: *"Danny's agility would
/// then be calculated as follows: base skill 4, –8 from chaos token,
/// +2 from "Lucky!" for a total of –2, which is still treated as
/// zero."* The clamp is last, so it must not swallow the +2 that
/// arrives after the −8.
#[test]
fn the_clamp_is_applied_once_after_every_modifier() {
    let breakdown = ModifierBreakdown {
        base: 4,
        contributions: vec![
            Contribution {
                source: ContributionSource::Recorded { instance: None },
                delta: -8,
            },
            Contribution {
                source: ContributionSource::Recorded { instance: None },
                delta: 2,
            },
        ],
        substitution: None,
    };
    assert_eq!(breakdown.total(), 0, "4 - 8 + 2 = -2, treated as 0");
    // The answer a fold that clamped between the −8 and the +2 would
    // give, spelled out so the assertion below still discriminates:
    // (4 − 8 → 0) + 2 = 2.
    let clamping_early = 4_i32.saturating_add(-8).max(0).saturating_add(2);
    assert_eq!(clamping_early, 2, "the wrong fold's arithmetic");
    assert_ne!(
        breakdown.total(),
        clamping_early,
        "clamping before the +2 would give 2 — the answer the rules \
         reference explicitly rules out"
    );
}

#[test]
fn a_breakdown_with_no_contributions_is_its_base() {
    let (state, id) = state_with_cards_in_play(&[]);
    let breakdown = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(id),
        ModifiedQuantity::Skill(SkillKind::Willpower),
        ReadContext::DuringTest(SkillTestKind::Plain),
    );
    assert_eq!(breakdown.base, 3, "test_investigator's printed willpower");
    assert!(breakdown.contributions.is_empty());
    assert_eq!(breakdown.total(), 3);
}

/// The breakdown names each contribution's source, so a client can
/// show why a value is what it is.
#[test]
fn a_breakdown_attributes_each_contribution_to_its_source() {
    let (state, id) = state_with_cards_in_play(&["willpower-plus-1", "willpower-minus-1"]);
    let breakdown = modified_value(
        &state,
        Some(&mock_registry()),
        ModifierTarget::Investigator(id),
        ModifiedQuantity::Skill(SkillKind::Willpower),
        ReadContext::DuringTest(SkillTestKind::Plain),
    );
    assert_eq!(breakdown.base, 3);
    assert_eq!(
        breakdown.contributions,
        vec![
            Contribution {
                source: ContributionSource::Card {
                    code: CardCode::new("willpower-plus-1"),
                    instance: Some(CardInstanceId(0)),
                },
                delta: 1,
            },
            Contribution {
                source: ContributionSource::Card {
                    code: CardCode::new("willpower-minus-1"),
                    instance: Some(CardInstanceId(1)),
                },
                delta: -1,
            },
        ],
    );
    assert_eq!(breakdown.total(), 3);
}

/// `elder_sign_expr` reads the controller's investigator card's
/// `Trigger::ElderSign { modifier }` and hands back the expression
/// itself. Roland's `Count(CluesAtControllerLocation)` evaluates to the
/// clue count at his location; an investigator with no elder-sign
/// ability has no expression to record at all.
#[test]
fn an_elder_sign_modifier_evaluates_its_expression() {
    let mut inv = test_support::test_investigator(1);
    inv.investigator_card.code = CardCode::new("elder-sign-clues-here");
    inv.current_location = Some(LocationId(10));
    let mut loc = test_support::test_location(10, "Study");
    loc.clues = 2;
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc)
        .build();
    let expr = elder_sign_expr(&state, &mock_registry(), InvestigatorId(1))
        .expect("the investigator card carries an elder-sign ability");
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    assert_eq!(evaluator::eval_int_expr(&state, &ctx, &expr), Ok(2));

    let mut plain = test_support::test_investigator(2);
    plain.investigator_card.code = CardCode::new("no-elder-sign");
    let state = GameStateBuilder::new().with_investigator(plain).build();
    assert_eq!(
        elder_sign_expr(&state, &mock_registry(), InvestigatorId(2)),
        None
    );
}
