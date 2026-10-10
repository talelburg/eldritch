use super::*;

/// Sequence composition: a hypothetical "gain 1 resource AND
/// discover 1 clue at your location" combined effect.
#[test]
fn seq_composition_nests_two_effects() {
    let effect = seq([
        gain_resources(InvestigatorTarget::You, 1),
        discover_clue(LocationTarget::YourLocation, 1),
    ]);
    match effect {
        Effect::Seq(inner) => assert_eq!(inner.len(), 2),
        _ => panic!("expected Seq"),
    }
}

/// `if_` and `if_else` build the same variant; only `else_` differs.
#[test]
fn conditional_branches_box_the_inner_effects() {
    let bare = if_(
        Condition::SkillTest {
            outcome: TestOutcome::Success,
        },
        discover_clue(LocationTarget::YourLocation, 1),
    );
    let with_else = if_else(
        Condition::SkillTest {
            outcome: TestOutcome::Success,
        },
        discover_clue(LocationTarget::YourLocation, 1),
        gain_resources(InvestigatorTarget::You, 1),
    );
    assert!(matches!(bare, Effect::If { else_: None, .. }));
    assert!(matches!(with_else, Effect::If { else_: Some(_), .. }));
}

/// `for_each` boxes its body and accepts a target-set spec.
#[test]
fn for_each_runs_body_per_target() {
    let effect = for_each(
        InvestigatorTargetSet::All,
        gain_resources(InvestigatorTarget::Active, 1),
    );
    assert!(matches!(
        effect,
        Effect::ForEach {
            targets: InvestigatorTargetSet::All,
            ..
        }
    ));
}

/// `choose_one` accepts an iterable like `seq`.
#[test]
fn choose_one_collects_alternatives() {
    let effect = choose_one([
        (
            "Gain 2 resources",
            gain_resources(InvestigatorTarget::You, 2),
        ),
        (
            "Discover 1 clue",
            discover_clue(LocationTarget::YourLocation, 1),
        ),
    ]);
    match effect {
        Effect::ChooseOne(alts) => {
            assert_eq!(alts.len(), 2);
            assert_eq!(alts[0].label, "Gain 2 resources");
            assert_eq!(alts[1].label, "Discover 1 clue");
        }
        _ => panic!("expected ChooseOne"),
    }
}

/// `InvestigatorTarget::You` and `Active` are distinct
/// variants — they coincide during the controller's own turn but
/// differ during reactions across turns. The compiler enforces
/// the difference at every match site; this test pins the
/// distinction at the type level.
#[test]
fn investigator_target_controller_and_active_are_distinct() {
    assert_ne!(InvestigatorTarget::You, InvestigatorTarget::Active);
    let controller_effect = gain_resources(InvestigatorTarget::You, 1);
    let active_effect = gain_resources(InvestigatorTarget::Active, 1);
    assert_ne!(controller_effect, active_effect);
}

/// A deeply-nested effect tree round-trips through `serde_json`.
/// Cheap insurance against `Box<Effect>` × nested-variant × serde
/// derive surprises.
#[test]
fn deeply_nested_effect_round_trips_through_serde_json() {
    let original = seq([
        if_else(
            Condition::SkillTest {
                outcome: TestOutcome::Success,
            },
            for_each(
                InvestigatorTargetSet::AtControllerLocation,
                gain_resources(InvestigatorTarget::Active, 1),
            ),
            modify(Stat::Intellect, -1, ModifierScope::ThisSkillTest),
        ),
        choose_one([
            (
                "Discover 1 clue",
                discover_clue(LocationTarget::YourLocation, 1),
            ),
            (
                "Gain 2 resources",
                gain_resources(InvestigatorTarget::You, 2),
            ),
        ]),
    ]);
    let json = serde_json::to_string(&original).expect("serialize");
    let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, recovered);
}

#[test]
fn native_effect_round_trips_through_serde_json() {
    let effect = native("01108:board-build");
    let json = serde_json::to_string(&effect).expect("serialize");
    let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(effect, recovered);
}

/// `Condition::Native` (#592) round-trips, and composes inside an
/// [`IntExpr::Cond`] — the position Machete 01020 uses it from.
#[test]
fn native_condition_round_trips_through_serde_json() {
    let expr = IntExpr::cond(native_condition("01020:sole-engaged-target"), 1, 0);
    assert!(matches!(
        &expr,
        IntExpr::Cond {
            when: Condition::Native { tag },
            then: 1,
            otherwise: 0,
        } if tag == "01020:sole-engaged-target"
    ));
    let json = serde_json::to_string(&expr).expect("serialize");
    let recovered: IntExpr = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(expr, recovered);
}

/// `Effect::BoostAttackDamage` (Vicious Blow 01025's "+1 damage")
/// round-trips through serde, and the builder constructs the variant.
#[test]
fn boost_attack_damage_round_trips_through_serde_json() {
    let effect = boost_attack_damage(1);
    assert_eq!(effect, Effect::BoostAttackDamage(1));
    let json = serde_json::to_string(&effect).expect("serialize");
    let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(effect, recovered);
}

/// `Effect::DiscoverAdditionalClues` (Deduction 01039's "1 additional
/// clue") round-trips through serde, and the builder constructs the
/// variant.
#[test]
fn discover_additional_clues_round_trips_through_serde_json() {
    let effect = discover_additional_clues(1);
    assert_eq!(effect, Effect::DiscoverAdditionalClues(1));
    let json = serde_json::to_string(&effect).expect("serialize");
    let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(effect, recovered);
}

#[test]
fn choose_surface_serde_round_trips() {
    let inv = InvestigatorTarget::chosen_anywhere();
    let loc = LocationTarget::chosen_anywhere();
    let here = InvestigatorTarget::Chosen(Choose {
        scope: EntityScope::At(LocationSet::Here),
    });
    for t in [inv, here] {
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(
            serde_json::from_str::<InvestigatorTarget>(&json).unwrap(),
            t
        );
    }
    let json = serde_json::to_string(&loc).unwrap();
    assert_eq!(serde_json::from_str::<LocationTarget>(&json).unwrap(), loc);
}

#[test]
fn heal_serde_round_trips() {
    let e = heal(
        HarmKind::Horror,
        InvestigatorTarget::chosen_at_your_location(),
        1,
    );
    assert_eq!(
        e,
        Effect::Heal {
            kind: HarmKind::Horror,
            target: InvestigatorTarget::chosen_at_your_location(),
            count: 1,
        }
    );
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(serde_json::from_str::<Effect>(&json).unwrap(), e);
}

#[test]
fn deal_builders_produce_the_kinded_effect_and_round_trip() {
    let dmg = deal_damage(InvestigatorTarget::You, 2u8);
    let hor = deal_horror(InvestigatorTarget::You, 3u8);
    assert_eq!(
        dmg,
        Effect::Deal {
            kind: HarmKind::Damage,
            target: InvestigatorTarget::You,
            amount: IntExpr::Lit(2),
        }
    );
    assert_eq!(
        hor,
        Effect::Deal {
            kind: HarmKind::Horror,
            target: InvestigatorTarget::You,
            amount: IntExpr::Lit(3),
        }
    );
    for e in [dmg, hor] {
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Effect>(&json).unwrap(), e);
    }
}

#[test]
fn deal_damage_to_enemy_serde_round_trips() {
    let e = deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1);
    assert_eq!(
        e,
        Effect::DealDamageToEnemy {
            target: EnemyTarget::Chosen(Choose {
                scope: EntityScope::At(LocationSet::Here)
            }),
            amount: 1,
        }
    );
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(serde_json::from_str::<Effect>(&json).unwrap(), e);
}

/// `Effect::DrawCards` (Guts/Perception/… "draw 1 card") round-trips
/// through serde, and the builder constructs the variant.
#[test]
fn draw_cards_round_trips_through_serde_json() {
    let effect = draw_cards(InvestigatorTarget::You, 1);
    assert_eq!(
        effect,
        Effect::DrawCards {
            target: InvestigatorTarget::You,
            count: 1,
        },
    );
    let json = serde_json::to_string(&effect).expect("serialize");
    let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(effect, recovered);
}

#[test]
fn search_deck_builder_and_serde_round_trip() {
    let e = search_deck(
        InvestigatorTarget::chosen_at_your_location(),
        SearchScope::Top(3),
        None,
    );
    assert!(matches!(
        e,
        Effect::SearchDeck {
            target: InvestigatorTarget::Chosen(_),
            scope: SearchScope::Top(3),
            filter: None,
        }
    ));
    let json = serde_json::to_string(&e).expect("serialize");
    let back: Effect = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(e, back);

    let filtered = search_deck(
        InvestigatorTarget::You,
        SearchScope::EntireDeck,
        Some(CardFilter {
            trait_: Some("Tome".into()),
            kind: Some(CardType::Asset),
        }),
    );
    let json = serde_json::to_string(&filtered).expect("serialize");
    assert_eq!(
        filtered,
        serde_json::from_str::<Effect>(&json).expect("deserialize")
    );
}

#[test]
fn barricade_dsl_variants_round_trip() {
    let attach = attach_self_to_location();
    assert_eq!(attach, Effect::AttachSelfToLocation);
    let block = restrict(Restriction::EnemyMovementBlocked);
    for e in [attach, block] {
        let json = serde_json::to_string(&e).expect("ser");
        assert_eq!(e, serde_json::from_str::<Effect>(&json).expect("de"));
    }
    let pat = EventPattern::LeftLocation;
    let json = serde_json::to_string(&pat).expect("ser");
    assert_eq!(
        pat,
        serde_json::from_str::<EventPattern>(&json).expect("de")
    );
}

/// Effects clone deeply (the recursive Box doesn't break Clone).
#[test]
fn deeply_nested_effect_clones() {
    let original = seq([
        if_else(
            Condition::SkillTest {
                outcome: TestOutcome::Success,
            },
            for_each(
                InvestigatorTargetSet::AtControllerLocation,
                gain_resources(InvestigatorTarget::Active, 1),
            ),
            modify(Stat::Intellect, -1, ModifierScope::ThisSkillTest),
        ),
        choose_one([
            (
                "Discover 1 clue",
                discover_clue(LocationTarget::YourLocation, 1),
            ),
            (
                "Gain 2 resources",
                gain_resources(InvestigatorTarget::You, 2),
            ),
        ]),
    ]);
    let cloned = original.clone();
    assert_eq!(original, cloned);
}
