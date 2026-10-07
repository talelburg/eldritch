use super::*;

/// The `Fight` follow-up deals `1 + extra_damage + bonus_attack_damage`,
/// reading the commit-time accumulator off the in-flight record
/// (Vicious Blow 01025). With `extra_damage: 1` (a weapon bonus) and
/// `bonus_attack_damage: 2`, the attack deals `1 + 1 + 2 = 4`.
#[test]
fn fight_follow_up_adds_bonus_attack_damage() {
    let inv = InvestigatorId(1);
    let mut enemy = test_support::test_enemy(7, "Goon");
    enemy.max_health = 10; // avoid clamping so the dealt damage is observable
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .build();
    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            follow_up: SkillTestFollowUp::Fight {
                enemy: EnemyId(7),
                extra_damage: 1,
            },
            bonus_attack_damage: 2,
            ..test_support::test_skill_test(
                SkillTestId(0),
                inv,
                SkillKind::Combat,
                SkillTestKind::Fight,
                2,
            )
        }));
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    apply_skill_test_follow_up(
        &mut cx,
        inv,
        SkillTestFollowUp::Fight {
            enemy: EnemyId(7),
            extra_damage: 1,
        },
    );

    assert_eq!(
        state.enemies[&EnemyId(7)].damage,
        4,
        "1 base + 1 extra_damage + 2 bonus_attack_damage"
    );
}

/// The `Investigate` follow-up pushes **one** `DiscoverClue` of
/// `1 + bonus_clues_discovered` at the test's `tested_location`, reading the
/// commit-time accumulator off the in-flight record (Deduction 01039). With
/// `bonus_clues_discovered: 1` that is a single discovery of 2 — not two of
/// 1, which is what Cover Up 01007 would replace twice (#471).
#[test]
fn investigate_follow_up_pushes_one_discovery_carrying_the_clue_bonus() {
    let inv = InvestigatorId(1);
    let loc = LocationId(10);
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), loc)
        .with_location(test_support::test_location(10, "Study"))
        .build();
    state
        .continuations
        .push(Continuation::SkillTest(InFlightSkillTest {
            tested_location: Some(loc),
            follow_up: SkillTestFollowUp::Investigate,
            bonus_clues_discovered: 1,
            ..test_support::test_skill_test(
                SkillTestId(0),
                inv,
                SkillKind::Intellect,
                SkillTestKind::Investigate,
                2,
            )
        }));
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    apply_skill_test_follow_up(&mut cx, inv, SkillTestFollowUp::Investigate);

    let Some(Continuation::Effect(EffectFrame::Leaf { effect, .. })) = state.continuations.top()
    else {
        panic!(
            "expected one pushed DiscoverClue leaf, got {:?}",
            state.continuations.top()
        );
    };
    assert_eq!(
        **effect,
        Effect::DiscoverClue {
            from: LocationTarget::TestedLocation,
            count: 2,
        },
        "one discovery of 1 base + 1 bonus, at the tested location",
    );
}

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
