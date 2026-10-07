use super::*;
use crate::state::EmitEventFrame;

#[test]
fn soak_and_place_with_no_soakers_matches_old_behavior() {
    // Regression guard for the assign/place/window rewrite: an attack
    // of 2 damage / 1 horror against an investigator controlling no
    // soak-bearing assets must land entirely on the investigator, just
    // as the pre-rewrite direct apply_damage/horror_numeric path did.
    test_support::install_test_registry();
    let id = InvestigatorId(1);
    let inv = test_support::test_investigator(1);
    // max_health()/max_sanity() now read from the registry (TEST_INV = 8/8).
    // 2 damage and 1 horror both land below capacity, so no defeat fires.
    // The old explicit max_health = 10 / max_sanity = 10 are vestigial.

    let mut state = GameStateBuilder::new().with_investigator(inv).build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    soak_and_place(&mut cx, id, 2, 1);

    assert_eq!(state.investigators[&id].damage(), 2, "all damage on inv");
    assert_eq!(state.investigators[&id].horror(), 1, "all horror on inv");
    assert_event!(events, Event::DamageTaken { investigator, amount: 2 } if *investigator == id);
    assert_event!(events, Event::HorrorTaken { investigator, amount: 1 } if *investigator == id);
    assert!(
        state.open_windows().is_empty(),
        "no soak window without soakers"
    );
}

#[test]
fn advance_distribution_drains_without_soakers_and_prompts_with_one() {
    // No soaker → fully deterministic: all damage to the investigator, drained.
    let mut asg = Assignment::default();
    let (mut d, mut h) = (2u8, 0u8);
    assert!(advance_distribution(&[], &mut d, &mut h, &mut asg).is_some());
    assert_eq!((d, h, asg.investigator_damage), (0, 0, 2));

    // A soaker with capacity → a damage point is contested → prompt (None),
    // and the counters still show the un-assigned points.
    let soaker = Soaker {
        instance: CardInstanceId(1),
        remaining_health: 3,
        remaining_sanity: 0,
    };
    let mut asg2 = Assignment::default();
    let (mut d2, mut h2) = (2u8, 0u8);
    assert!(advance_distribution(&[soaker], &mut d2, &mut h2, &mut asg2).is_none());
    assert_eq!(
        (d2, h2),
        (2, 0),
        "nothing auto-assigned while a soaker can take the point"
    );
}

#[test]
fn resume_damage_distribution_rejects_invalid_pick_and_keeps_frame() {
    let inv_id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    // Park a DealDamage frame mid-distribution (2 damage to assign).
    state.continuations.push(DealDamageFrame {
        investigator: inv_id,
        source: DamageSource::EnemyAttack { enemy: EnemyId(7) },
        assignment: Assignment::default(),
        step: DealDamageStep::Distribute {
            remaining_damage: 2,
            remaining_horror: 0,
        },
    });
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    // Wrong response variant → reject, frame untouched.
    let wrong = resume_damage_distribution(&mut cx, &InputResponse::Skip);
    assert!(matches!(wrong, EngineOutcome::Rejected { .. }));

    // Out-of-range option (no soakers → only the investigator is eligible,
    // so any index ≥ 1 is invalid) → reject, frame untouched.
    let oob = resume_damage_distribution(&mut cx, &InputResponse::PickSingle(OptionId(5)));
    assert!(matches!(oob, EngineOutcome::Rejected { .. }));

    // The frame survives both rejections, at the same step, for the client
    // to retry — a rejection must not advance the cursor.
    assert!(
        matches!(
            state.continuations.top(),
            Some(Continuation::DealDamage(DealDamageFrame {
                step: DealDamageStep::Distribute {
                    remaining_damage: 2,
                    remaining_horror: 0
                },
                ..
            }))
        ),
        "DealDamage{{Distribute}} frame retained after invalid picks"
    );
}

/// The cursor's whole walk (#727): `Distribute → Announce → Place → Finish`,
/// stepped one dispatch at a time with no soaker in play, so distribution
/// drains immediately and the frame never prompts.
///
/// What it pins is *where the placement is*: nothing is on the investigator
/// until the `DamagePlaced` coordinator the `Place` step pushes has reached
/// its own resolve step. The `when` cell between the two conditions is
/// exactly the gap this walk opens (ADR 0009).
#[test]
fn deal_damage_cursor_walks_distribute_announce_place_finish() {
    test_support::install_test_registry();
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    // Entry parks the frame at the top of the cursor and returns `Done`:
    // dealing damage is tail position, like an emit.
    let out = begin_deal_damage(
        &mut cx,
        id,
        2,
        0,
        DamageSource::EnemyAttack { enemy: EnemyId(7) },
    );
    assert_eq!(out, EngineOutcome::Done);
    assert!(matches!(
        cx.state.continuations.top(),
        Some(Continuation::DealDamage(DealDamageFrame {
            step: DealDamageStep::Distribute { .. },
            ..
        }))
    ));

    // Distribute: no soaker can take a point, so it drains without
    // prompting and the cursor reaches `Announce` with the whole 2 on the
    // investigator's share.
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    let DealDamageFrame {
        assignment, step, ..
    } = cx.state.continuations.top_expect::<DealDamageFrame>();
    assert_eq!(*step, DealDamageStep::Announce);
    assert_eq!(assignment.investigator_damage, 2);

    // Announce: the cursor advances *before* the emit (tail position), and
    // the coordinator it pushed is now on top.
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    assert!(matches!(
        cx.state.continuations.top(),
        Some(Continuation::EmitEvent(EmitEventFrame {
            event: TimingEvent::DamageAssigned { .. },
            ..
        }))
    ));
    assert_eq!(
        cx.state.investigators[&id].damage(),
        0,
        "an assignment is a proposal: nothing is placed at the announcement"
    );
    // Drain that coordinator (a bare milestone with no takers here).
    cx.state.continuations.pop();

    // Place: same shape, and again nothing has landed until the coordinator
    // reaches its resolve step.
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    let Some(Continuation::EmitEvent(EmitEventFrame {
        event: placed @ TimingEvent::DamagePlaced { .. },
        ..
    })) = cx.state.continuations.top().cloned()
    else {
        panic!("Place must emit DamagePlaced");
    };
    assert_eq!(
        cx.state.investigators[&id].damage(),
        0,
        "still nothing placed until DamagePlaced resolves"
    );
    cx.state.continuations.pop();
    // The resolve step is where it lands.
    let resolution = placed.condition_resolution();
    let ConditionResolution::Coordinator(resolve) = resolution else {
        panic!("DamagePlaced must be coordinator-owned");
    };
    assert_eq!(resolve(&mut cx, &placed), EngineOutcome::Done);
    assert_eq!(cx.state.investigators[&id].damage(), 2, "placed at last");
    assert!(
        cx.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 2 } if *investigator == id
        )),
        "DamageTaken emitted at the placement: {:?}",
        cx.events
    );

    // Finish: pop and hand back to the caller. An enemy attack's own
    // sequence continues on the frames beneath, so this is just `Done`.
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    assert!(
        !cx.state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::DealDamage(_))),
        "the frame pops at Finish"
    );
}

/// `DamageAssigned` is the **bare milestone** of the pair and `DamagePlaced`
/// the one with an impact (ADR 0008's classification, ADR 0009's split).
/// Asserted directly, because the whole `when`-cell licence Guard Dog needs
/// rests on the first of those being true.
#[test]
fn the_two_damage_conditions_are_classified_as_the_adr_says() {
    let assigned = TimingEvent::DamageAssigned {
        source: DamageSource::EnemyAttack { enemy: EnemyId(1) },
        investigator: InvestigatorId(1),
        assignment: Assignment::default(),
    };
    let placed = TimingEvent::DamagePlaced {
        source: DamageSource::EnemyAttack { enemy: EnemyId(1) },
        investigator: InvestigatorId(1),
        assignment: Assignment::default(),
    };
    // Both coordinator-owned, so both walk their `when` cell.
    for event in [&assigned, &placed] {
        assert!(
            matches!(
                event.condition_resolution(),
                ConditionResolution::Coordinator(_)
            ),
            "{event:?} must be coordinator-owned"
        );
    }
    // The milestone's resolve step changes nothing; the other's places.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let before = state.clone();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let ConditionResolution::Coordinator(resolve) = assigned.condition_resolution() else {
        unreachable!()
    };
    assert_eq!(resolve(&mut cx, &assigned), EngineOutcome::Done);
    assert_eq!(*cx.state, before, "a bare milestone resolves to nothing");
    assert!(events.is_empty());
    // Neither fires forced abilities: every forced taker in the sweep is
    // outside the corpus (ADR 0009).
    assert!(assigned.forced_point().is_none());
    assert!(placed.forced_point().is_none());
}

#[test]
fn soak_options_anchor_assets_to_card_instances() {
    let targets = vec![
        DistributionTarget::Investigator,
        DistributionTarget::Asset(CardInstanceId(7)),
    ];
    let me = OptionTarget::CardInstance(CardInstanceId(1));
    let opts = soak_options(&targets, &me);
    // Anchors: the investigator to their own card; a soaker asset to its card.
    assert_eq!(opts[0].target, Some(me), "the investigator's own card");
    assert_eq!(
        opts[1].target,
        Some(OptionTarget::CardInstance(CardInstanceId(7)))
    );
    // Labels unchanged from the former `hunters::candidate_options` debug repr.
    assert_eq!(opts[0].label, "Investigator");
    assert_eq!(opts[1].label, "Asset(CardInstanceId(7))");
}
