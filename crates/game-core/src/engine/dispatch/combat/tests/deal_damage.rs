use super::*;
use crate::state::EmitEventFrame;
use crate::test_support::SeatLookup as _;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

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
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    // Park a DealDamage frame mid-distribution (2 damage to assign).
    state.continuations.push(DealDamageFrame {
        investigator: inv_id,
        source: DamageSource::EnemyAttack { enemy: EnemyId(7) },
        direct: false,
        assignment: Assignment::default(),
        step: DealDamageStep::Distribute {
            remaining_damage: 2,
            remaining_horror: 0,
        },
    });
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);

    // Wrong response variant → reject, frame untouched.
    let wrong = resume_damage_distribution(&mut cx, &InputResponse::Skip);
    assert!(matches!(wrong, EngineOutcome::Rejected { .. }));

    // Out-of-range option (no soakers → only the investigator is eligible,
    // so any index ≥ 1 is invalid) → reject, frame untouched. A malformed
    // response no step can name, so the resume handler is called with it raw.
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

/// Run the resolve step of the coordinator-owned `event`, as the coordinator
/// does once its `when` cell has run.
fn resolve_condition(cx: &mut Cx, event: &TimingEvent) -> EngineOutcome {
    let ConditionResolution::Coordinator(resolve) = event.condition_resolution() else {
        panic!("{event:?} must be coordinator-owned");
    };
    resolve(cx, event)
}

/// The `TimingEvent` on the top `EmitEvent` frame, or a panic naming `what`.
fn top_emitted(cx: &Cx, what: &str) -> TimingEvent {
    let Some(Continuation::EmitEvent(EmitEventFrame { event, .. })) = cx.state.continuations.top()
    else {
        panic!(
            "expected {what} on top; stack = {:?}",
            cx.state.continuations
        );
    };
    event.clone()
}

/// The cursor's whole walk: `Distribute → Announce → Finish`,
/// stepped one dispatch at a time with no soaker in play, so distribution
/// drains immediately and the frame never prompts.
///
/// What it pins is *where the placement is*: nothing is on the investigator
/// until `DamageDealt`'s resolve step has emitted `DamagePlaced` and that
/// condition has reached its own resolve step. The frame sits at `Finish`
/// beneath both, so the placement nests inside the deal (ADR 0009).
#[test]
fn deal_damage_cursor_walks_distribute_announce_finish() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);

    // Entry parks the frame at the top of the cursor and returns `Done`:
    // dealing damage is tail position, like an emit.
    let out = deal_harm(
        &mut cx,
        id,
        2,
        0,
        DamageSource::EnemyAttack { enemy: EnemyId(7) },
        false,
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

    // Announce: the cursor advances straight to `Finish` *before* the emit
    // (tail position), and the `DamageDealt` coordinator is now on top.
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    let dealt = top_emitted(&cx, "DamageDealt");
    assert!(
        matches!(dealt, TimingEvent::DamageDealt { .. }),
        "{dealt:?}"
    );
    assert_eq!(
        cx.state
            .continuations
            .topmost_of::<DealDamageFrame>()
            .map(|f| &f.step),
        Some(&DealDamageStep::Finish),
        "the frame waits beneath the whole sequence at Finish"
    );
    assert_eq!(
        cx.state.investigators[&id].damage(),
        0,
        "an assignment is a proposal: nothing is placed at the announcement"
    );

    // DamageDealt's resolve step: emit DamagePlaced above it, still placing
    // nothing.
    assert_eq!(resolve_condition(&mut cx, &dealt), EngineOutcome::Done);
    let placed = top_emitted(&cx, "DamagePlaced");
    assert!(
        matches!(placed, TimingEvent::DamagePlaced { .. }),
        "{placed:?}"
    );
    assert_eq!(
        cx.state.investigators[&id].damage(),
        0,
        "still nothing placed until DamagePlaced resolves"
    );

    // DamagePlaced's resolve step is where it lands.
    cx.state.continuations.pop();
    assert_eq!(resolve_condition(&mut cx, &placed), EngineOutcome::Done);
    assert_eq!(cx.state.investigators[&id].damage(), 2, "placed at last");
    assert_event!(cx.events, Event::DamagePlacedOnInvestigator { investigator, amount: 2 } if *investigator == id);

    // Finish: pop and hand back to the caller. An enemy attack's own
    // sequence continues on the frames beneath, so this is just `Done`.
    cx.state.continuations.pop();
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    assert!(
        !cx.state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::DealDamage(_))),
        "the frame pops at Finish"
    );
}

/// The re-read ADR 0009 relies on: `DamageDealt`'s resolve step hands
/// `DamagePlaced` the frame's **live** assignment, not the one `DamageDealt`
/// announced. A `when`-cell edit — partial damage prevention, the one thing
/// the window exists for — therefore reaches the placement.
#[test]
fn damage_dealt_places_the_live_assignment_not_the_announced_one() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    assert_eq!(
        deal_harm(&mut cx, id, 2, 0, DamageSource::Effect, false),
        EngineOutcome::Done
    );
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    let dealt = top_emitted(&cx, "DamageDealt");

    // What a `when`-cell edit would do: shrink the frame's live share.
    cx.state
        .continuations
        .topmost_of_mut::<DealDamageFrame>()
        .expect("the deal is parked beneath")
        .assignment
        .investigator_damage = 1;

    assert_eq!(resolve_condition(&mut cx, &dealt), EngineOutcome::Done);
    let TimingEvent::DamagePlaced { assignment, .. } = top_emitted(&cx, "DamagePlaced") else {
        unreachable!()
    };
    assert_eq!(
        assignment.investigator_damage, 1,
        "DamagePlaced carries the edited assignment, not the announced 2"
    );
}

/// Both conditions are coordinator-owned with an impact (ADR 0008's
/// classification, ADR 0009's nesting): `DamageDealt`'s resolve step emits
/// `DamagePlaced`, and `DamagePlaced`'s places. Neither is a bare milestone.
#[test]
fn the_two_damage_conditions_are_classified_as_the_adr_says() {
    let source = DamageSource::EnemyAttack { enemy: EnemyId(1) };
    let assignment = Assignment {
        investigator_horror: 1,
        ..Assignment::default()
    };
    let dealt = TimingEvent::DamageDealt {
        source,
        investigator: InvestigatorId(1),
        assignment: assignment.clone(),
    };
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    state.continuations.push(DealDamageFrame {
        investigator: InvestigatorId(1),
        source,
        direct: false,
        assignment: assignment.clone(),
        step: DealDamageStep::Finish,
    });
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);

    // The deal's impact is the placement's whole sequence: its resolve step
    // emits `DamagePlaced` and places nothing itself.
    assert_eq!(resolve_condition(&mut cx, &dealt), EngineOutcome::Done);
    let placed = top_emitted(&cx, "DamagePlaced");
    assert_eq!(
        placed,
        TimingEvent::DamagePlaced {
            source,
            investigator: InvestigatorId(1),
            assignment,
        }
    );
    assert_eq!(cx.state.investigators[&InvestigatorId(1)].horror(), 0);

    // The placement's impact is the tokens landing.
    cx.state.continuations.pop();
    assert_eq!(resolve_condition(&mut cx, &placed), EngineOutcome::Done);
    assert_eq!(cx.state.investigators[&InvestigatorId(1)].horror(), 1);
}

#[test]
fn soak_options_anchor_assets_to_card_instances() {
    let state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).in_play([test_support::TEST_ASSET]))
        .build();
    let asset = state.in_play(InvestigatorId(1), test_support::TEST_ASSET);
    let targets = vec![
        DistributionTarget::Investigator,
        DistributionTarget::Asset(asset),
    ];
    let me = state.investigators[&InvestigatorId(1)].card_anchor();
    let opts = soak_options(&state, &TEST_REGISTRY, &targets, &me);
    // Anchors: the investigator to their own card; a soaker asset to its card.
    assert_eq!(opts[0].target, Some(me), "the investigator's own card");
    assert_eq!(opts[1].target, Some(OptionTarget::CardInstance(asset)));
    // Labels: each anchored card's name, not the target's Debug form.
    assert_eq!(opts[0].label, "Test Investigator");
    assert_eq!(opts[1].label, "Test Asset");
}

/// The one harm adapter deals through the same cursor an enemy attack
/// does: `deal_harm` parks a `DealDamage { source: Effect }` frame, and
/// the walk announces `DamageDealt`, whose resolve step announces
/// `DamagePlaced`, before anything lands. Walked once here on the kernel
/// adapter; each caller (Crypt Chill, the deck-out penalty, Dynamite Blast) only
/// asserts that it pushes this frame.
#[test]
fn deal_harm_walks_distribute_announce_finish_from_an_effect() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);

    assert_eq!(
        deal_harm(&mut cx, id, 0, 1, DamageSource::Effect, false),
        EngineOutcome::Done
    );
    test_support::assert_effect_deal(cx.state.continuations.top(), id, 0, 1);

    // Distribute drains (no soaker), then Announce emits DamageDealt.
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    let dealt = top_emitted(&cx, "DamageDealt");
    assert!(
        matches!(
            dealt,
            TimingEvent::DamageDealt {
                source: DamageSource::Effect,
                ..
            }
        ),
        "{dealt:?}"
    );
    assert_eq!(cx.state.investigators[&id].horror(), 0);

    // DamageDealt's resolve step emits DamagePlaced; the horror lands at that
    // condition's own resolve step.
    assert_eq!(resolve_condition(&mut cx, &dealt), EngineOutcome::Done);
    let placed = top_emitted(&cx, "DamagePlaced");
    assert_eq!(cx.state.investigators[&id].horror(), 0);
    cx.state.continuations.pop();
    assert_eq!(resolve_condition(&mut cx, &placed), EngineOutcome::Done);
    assert_eq!(cx.state.investigators[&id].horror(), 1, "placed at last");
    assert_event!(cx.events, Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == id);

    // Finish pops the frame and hands back `Done`: nothing to resume.
    cx.state.continuations.pop();
    assert_eq!(drive_deal_damage(&mut cx), EngineOutcome::Done);
    assert!(
        cx.state.continuations.top().is_none(),
        "the frame pops at Finish"
    );
}

/// No harm, no deal: `deal_harm(.., 0, 0)` pushes nothing on any path (an
/// effect, an enemy attack, a direct deal), so no condition announces an
/// empty assignment.
#[test]
fn deal_harm_of_nothing_pushes_no_frame() {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    for (source, direct) in [
        (DamageSource::Effect, false),
        (DamageSource::EnemyAttack { enemy: EnemyId(7) }, false),
        (DamageSource::Effect, true),
    ] {
        assert_eq!(
            deal_harm(&mut cx, InvestigatorId(1), 0, 0, source, direct),
            EngineOutcome::Done
        );
        assert!(
            cx.state.continuations.top().is_none(),
            "{source:?}, direct: {direct}"
        );
    }
}
