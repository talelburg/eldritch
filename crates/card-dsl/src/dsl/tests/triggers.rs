use super::*;

/// Deduction-shaped commit-trigger ability. The DSL won't fully
/// resolve this until the engine grows commit-time machinery in
/// Phase 3, but the type-level construction has to work today so
/// future commit-trigger cards have somewhere to land.
#[test]
fn on_commit_distinct_from_on_play() {
    let ability = on_commit(if_(
        Condition::SkillTest {
            outcome: TestOutcome::Success,
        },
        discover_clue(LocationTarget::YourLocation, 1),
    ));
    assert_eq!(ability.trigger, Trigger::OnCommit);
    // Distinct enum variant — compiler enforces the difference at
    // every match site, which is the whole point of separating
    // them rather than reusing OnPlay.
    assert_ne!(ability.trigger, Trigger::OnPlay);
}

/// `on_skill_test_resolution` builds the outcome-gated trigger
/// and accepts `TestedLocation`-targeted effects.
#[test]
fn on_skill_test_resolution_builder() {
    let ability = on_skill_test_resolution(
        TestOutcome::Success,
        discover_clue(LocationTarget::TestedLocation, 1),
    );
    assert_eq!(
        ability.trigger,
        Trigger::OnSkillTestResolution {
            outcome: TestOutcome::Success,
        },
    );
    assert!(matches!(
        ability.effect,
        Effect::DiscoverClue {
            from: LocationTarget::TestedLocation,
            count: 1,
        },
    ));
    assert!(ability.costs.is_empty());
}

/// Roland-Banks-shaped reaction: "after you defeat an enemy,
/// discover 1 clue at your location" — the canonical motivating
/// card for [`Trigger::OnEvent`]. The DSL doesn't fire it yet
/// (engine reaction windows land in #52), but construction must
/// work today so #55 has somewhere to land.
#[test]
fn on_event_builder_constructs_roland_banks_reaction() {
    let ability = on_event(
        EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        },
        EventTiming::After,
        TriggerKind::Reaction,
        discover_clue(LocationTarget::YourLocation, 1),
    );
    assert_eq!(
        ability.trigger,
        Trigger::OnEvent {
            pattern: EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            timing: EventTiming::After,
            kind: TriggerKind::Reaction,
        },
    );
    assert!(matches!(
        ability.effect,
        Effect::DiscoverClue {
            from: LocationTarget::YourLocation,
            count: 1,
        },
    ));
    assert!(ability.costs.is_empty());
}

/// `OnEvent` is a distinct enum variant from existing trigger
/// shapes — the compiler enforces the distinction at every match
/// site, and the `by_controller` / `timing` fields differentiate
/// the currently-expressible sub-cases. Pattern-vs-pattern
/// distinction lands as soon as a second [`EventPattern`] variant
/// arrives.
#[test]
fn on_event_distinct_from_other_triggers_and_internally() {
    let after_any = Trigger::OnEvent {
        pattern: EventPattern::EnemyDefeated {
            by_controller: false,
            code: None,
        },
        timing: EventTiming::After,
        kind: TriggerKind::Reaction,
    };
    let after_controller = Trigger::OnEvent {
        pattern: EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        },
        timing: EventTiming::After,
        kind: TriggerKind::Reaction,
    };
    let when_controller = Trigger::OnEvent {
        pattern: EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        },
        timing: EventTiming::When,
        kind: TriggerKind::Reaction,
    };
    assert_ne!(after_any, Trigger::Constant);
    assert_ne!(after_any, Trigger::OnPlay);
    assert_ne!(after_any, Trigger::OnCommit);
    assert_ne!(after_any, after_controller);
    assert_ne!(after_controller, when_controller);
}

/// An `OnEvent`-triggered ability round-trips through `serde_json`
/// — struct-variant × serde derive can surprise; pin the wire
/// shape now so #52's persistence doesn't re-discover problems
/// later. All three [`EventTiming`] variants (`When`, `At`, `After`)
/// are exercised independently since unit-variant × serde can fail on
/// any alone (very rare, but the test rationale explicitly covers this
/// surface).
#[test]
fn on_event_ability_round_trips_through_serde_json() {
    for timing in [EventTiming::When, EventTiming::At, EventTiming::After] {
        let original = on_event(
            EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            timing,
            TriggerKind::Reaction,
            discover_clue(LocationTarget::YourLocation, 1),
        );
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Ability = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }
}

/// `Trigger::ElderSign` is a config-on-trigger variant (like
/// `Activated { action_cost }`): it carries the elder-sign's printed
/// modifier as an `IntExpr` and round-trips through serde. Roland's
/// "+1 for each clue on your location" is `Count(CluesAtControllerLocation)`.
#[test]
fn elder_sign_trigger_carries_int_expr_and_round_trips() {
    let t = Trigger::ElderSign {
        modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
    };
    let json = serde_json::to_string(&t).expect("serialize");
    let back: Trigger = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(t, back);
    // Distinct from a literal-modifier elder-sign and from other triggers.
    assert_ne!(
        t,
        Trigger::ElderSign {
            modifier: IntExpr::Lit(1),
        },
    );
    assert_ne!(t, Trigger::Constant);
}

/// The `elder_sign` builder produces an [`Ability`] with the correct
/// trigger, empty costs, empty effect `Seq`, and no usage limit.
/// Distinct from the `elder_sign_trigger_carries_int_expr_and_round_trips`
/// test which only exercises the `Trigger` variant itself.
#[test]
fn elder_sign_builder_constructs_the_trigger() {
    let a = elder_sign(IntExpr::Count(Quantity::CluesAtControllerLocation));
    assert_eq!(
        a.trigger,
        Trigger::ElderSign {
            modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
        },
    );
    assert!(a.costs.is_empty());
    assert!(a.usage_limit.is_none());
    assert!(matches!(a.effect, Effect::Seq(ref v) if v.is_empty()));
}

/// `Trigger::OnEvent` carries an explicit `TriggerKind` (forced vs
/// reaction), and it round-trips through serde. The kind retires the
/// old route-by-pattern dispatch (umbrella §2, Axis-B T1).
#[test]
fn on_event_carries_trigger_kind() {
    let t = Trigger::OnEvent {
        pattern: EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        },
        timing: EventTiming::After,
        kind: TriggerKind::Reaction,
    };
    let json = serde_json::to_string(&t).expect("serialize");
    let back: Trigger = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(t, back);
    // Forced and Reaction are distinct.
    assert_ne!(
        t,
        Trigger::OnEvent {
            pattern: EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            timing: EventTiming::After,
            kind: TriggerKind::Forced,
        },
    );
}

/// The `revelation` builder produces the new Trigger variant with
/// the given effect. Distinct from `OnPlay` / `OnCommit` at the type
/// level so the compiler enforces the difference at every match site.
#[test]
fn revelation_builder_constructs_treachery_shape() {
    let ability = revelation(gain_resources(InvestigatorTarget::You, 1));
    assert_eq!(ability.trigger, Trigger::Revelation);
    assert!(matches!(
        ability.effect,
        Effect::GainResources {
            target: InvestigatorTarget::You,
            amount: 1,
        },
    ));
    assert!(ability.costs.is_empty());
    assert!(ability.usage_limit.is_none());
}

#[test]
fn revelation_distinct_from_other_triggers() {
    assert_ne!(Trigger::Revelation, Trigger::OnPlay);
    assert_ne!(Trigger::Revelation, Trigger::OnCommit);
    assert_ne!(Trigger::Revelation, Trigger::Constant);
}

#[test]
fn revelation_ability_round_trips_through_serde_json() {
    let original = revelation(gain_resources(InvestigatorTarget::You, 1));
    let json = serde_json::to_string(&original).expect("serialize");
    let recovered: Ability = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, recovered);
}
