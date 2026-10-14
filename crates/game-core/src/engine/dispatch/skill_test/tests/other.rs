use super::*;
use crate::test_support::TestSession;

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
        .with_seat(test_support::seat(1)) // card_code = "" sentinel
        .open_turn(inv)
        .build();
    // Willpower 3, difficulty 2, ElderSign token. Bonus 0 → total 3 → succeed by 1.
    state.chaos_bag.tokens = vec![ChaosToken::ElderSign];
    let session = TestSession::new(state)
        .test_skill(inv, SkillKind::Willpower, 2)
        .commit(&[]);
    session.assert_at_turn_menu(inv);
    crate::assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin, .. } if *margin == 1
    );
}
