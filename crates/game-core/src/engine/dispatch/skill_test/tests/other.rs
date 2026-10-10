use super::*;

#[test]
fn symbol_effects_to_effect_builds_deal_seq() {
    // Empty → nothing to push.
    assert_eq!(symbol_effects_to_effect(&[]), None);

    // Single effect → a bare Deal targeting the tester (interactive soak).
    assert_eq!(
        symbol_effects_to_effect(&[TokenEffect::Horror(1)]),
        Some(Effect::Deal {
            kind: HarmKind::Horror,
            target: InvestigatorTarget::You,
            amount: IntExpr::Lit(1),
        }),
    );

    // Multiple → one Seq, in order.
    assert!(matches!(
        symbol_effects_to_effect(&[TokenEffect::Damage(1), TokenEffect::Horror(2)]),
        Some(Effect::Seq(v)) if v.len() == 2
    ));
}

/// With no elder-sign ability on the controller's card (empty sentinel
/// `card_code`), the `ElderSign` token resolves exactly as before: total =
/// clamped skill value, bonus 0. Locks the behaviour-preserving default.
#[test]
fn elder_sign_token_adds_zero_without_an_elder_sign_ability() {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1)) // card_code = "" sentinel
        .with_active_investigator(inv)
        .build();
    // Willpower 3, difficulty 2, ElderSign token. Bonus 0 → total 3 → succeed by 1.
    state.chaos_bag.tokens = vec![ChaosToken::ElderSign];
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
    assert!(matches!(out, EngineOutcome::AwaitingInput { .. }));
    let out = finish_skill_test(&mut cx, &[]);
    let out = dispatch::drive(&mut cx, out);
    assert_eq!(out, EngineOutcome::Done);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::SkillTestSucceeded { margin, .. } if *margin == 1
        )),
        "ElderSign with no elder-sign ability → bonus 0 → succeed by 1: {events:?}",
    );
}
