use super::*;

/// The designators `ActionClass` names map onto it; the three it
/// doesn't name map to `None` (#754).
#[test]
fn a_designator_maps_onto_the_action_class_it_performs() {
    assert_eq!(fight(0u8, 0u8).action_class(), Some(ActionClass::Fight));
    assert_eq!(evade().action_class(), Some(ActionClass::Evade));
    // Cunning Distraction 01078's test-free Evade is an Evade all the same.
    assert_eq!(
        ActionDesignator::AutomaticEvade.action_class(),
        Some(ActionClass::Evade)
    );
    assert_eq!(
        ActionDesignator::Move.action_class(),
        Some(ActionClass::Move)
    );
    for d in [
        investigate(0u8),
        ActionDesignator::Parley,
        ActionDesignator::Resign,
    ] {
        assert_eq!(d.action_class(), None, "{d:?} names no ActionClass");
    }
}

/// The designator is the ability's, and only two builders set it (D21 of
/// #777): `activated_as` on an `Activated` ability and `on_play_as` on an
/// event's `OnPlay` ability. Every other builder leaves it `None`, so a
/// designator on a reaction, constant, commit, revelation or elder-sign
/// ability is not something a card can build.
#[test]
fn only_the_activated_and_on_play_builders_carry_a_designator() {
    let parlor = activated_as(ActionDesignator::Resign, 1, vec![], seq([]));
    assert_eq!(parlor.trigger, Trigger::Activated { action_cost: 1 });
    assert_eq!(parlor.designator, Some(ActionDesignator::Resign));

    // Blinding Light 01066: "<b>Evade.</b> This evasion attempt uses
    // [willpower] instead of [agility]. …"
    let played = on_play_as(evade(), seq([]));
    assert_eq!(played.trigger, Trigger::OnPlay);
    assert_eq!(played.designator, Some(evade()));
    assert!(played.costs.is_empty());

    for undesignated in [
        constant(seq([])),
        on_play(seq([])),
        on_commit(seq([])),
        on_skill_test_resolution(TestOutcome::Success, seq([])),
        reaction_on_event(EventPattern::RoundEnded, EventTiming::After, seq([])),
        forced_on_event(EventPattern::RoundEnded, EventTiming::After, seq([])),
        revelation(seq([])),
        activated(1, vec![], seq([])),
        elder_sign(IntExpr::Lit(1)),
    ] {
        assert_eq!(
            undesignated.designator, None,
            "{:?} prints no designator",
            undesignated.trigger,
        );
    }
}

#[test]
fn with_eligibility_sets_the_condition_and_default_is_none() {
    let bare = reaction_on_event(EventPattern::RoundEnded, EventTiming::When, Effect::Cancel);
    assert_eq!(bare.eligibility, None);
    let gated = reaction_on_event(EventPattern::RoundEnded, EventTiming::When, Effect::Cancel)
        .with_eligibility(native_condition("01109:can-advance"));
    assert_eq!(
        gated.eligibility,
        Some(native_condition("01109:can-advance"))
    );
}

/// Holy Rosary's "while in play, +1 willpower" ability.
#[test]
fn holy_rosary_willpower_modifier_compiles() {
    let ability = constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay));
    assert_eq!(ability.trigger, Trigger::Constant);
    assert!(matches!(
        ability.effect,
        Effect::Modify {
            stat: Stat::Willpower,
            delta: 1,
            scope: ModifierScope::WhileInPlay,
            audience: ModifierAudience::Controller,
        }
    ));
}

/// A multi-ability card naturally expressed as two separate
/// `Ability` declarations: one constant willpower modifier plus a
/// constant max-health buff. Illustrative shape only — not a real
/// printed card. (Holy Rosary's `sanity: 2` is *horror-soak*
/// capacity, NOT a max-sanity modifier; that's a redirect-and-
/// discard mechanic the DSL doesn't yet model — see #44.)
#[test]
fn vec_of_abilities_supports_multiple_constant_modifiers() {
    let abilities = [
        constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay)),
        constant(modify(Stat::MaxHealth, 1, ModifierScope::WhileInPlay)),
    ];
    assert_eq!(abilities.len(), 2);
    assert!(matches!(
        abilities[0].effect,
        Effect::Modify {
            stat: Stat::Willpower,
            delta: 1,
            scope: ModifierScope::WhileInPlay,
            audience: ModifierAudience::Controller,
        }
    ));
    assert!(matches!(
        abilities[1].effect,
        Effect::Modify {
            stat: Stat::MaxHealth,
            delta: 1,
            scope: ModifierScope::WhileInPlay,
            audience: ModifierAudience::Controller,
        }
    ));
}

/// Working a Hunch's "fast event: discover 1 clue at your location"
/// — the canonical `OnPlay` + `DiscoverClue` shape.
#[test]
fn working_a_hunch_compiles() {
    let ability = on_play(discover_clue(LocationTarget::YourLocation, 1));
    assert_eq!(ability.trigger, Trigger::OnPlay);
    assert!(matches!(
        ability.effect,
        Effect::DiscoverClue {
            from: LocationTarget::YourLocation,
            count: 1,
        }
    ));
}

#[test]
fn discard_self_cost_serde_round_trips() {
    let c = Cost::DiscardSelf;
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Cost>(&json).unwrap(), c);
}

#[test]
fn omitting_any_required_ability_field_is_rejected() {
    // `costs` is required on the wire (#453): a payload missing it fails
    // loudly rather than silently defaulting. `usage_limit` is an `Option`
    // and stays implicitly optional (handled separately below).
    let ability = on_event(
        EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        },
        EventTiming::When,
        TriggerKind::Reaction,
        discover_clue(LocationTarget::YourLocation, 1),
    );
    let full = serde_json::to_value(&ability).expect("serialize");
    serde_json::from_value::<Ability>(full.clone()).expect("full object deserializes");
    // `costs` is required; omitting it is rejected, not defaulted.
    let mut v = full.clone();
    v.as_object_mut()
        .expect("ability serializes to a JSON object")
        .remove("costs")
        .expect("`costs` present in serialized form");
    assert!(
        serde_json::from_value::<Ability>(v).is_err(),
        "omitting `costs` must be rejected, not defaulted"
    );
    // `usage_limit` stays implicitly optional (it is an `Option`; serde
    // defaults a missing one to `None`) — by design, see its doc.
    let mut v = full;
    v.as_object_mut()
        .unwrap()
        .remove("usage_limit")
        .expect("present in serialized form");
    let back = serde_json::from_value::<Ability>(v).expect("absent Option deserializes to None");
    assert!(back.usage_limit.is_none());
}
