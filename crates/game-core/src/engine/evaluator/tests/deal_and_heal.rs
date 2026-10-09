use super::*;

#[test]
fn heal_reduces_horror_saturating_and_emits_event() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .investigator_card
        .accumulated_horror = 1;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        // heal 2 from a 1-horror investigator → saturates to 0, amount 1.
        &dsl::heal(HarmKind::Horror, InvestigatorTarget::You, 2),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 0);
    assert_event!(
        events,
        Event::Healed {
            investigator: InvestigatorId(1),
            kind: HarmKind::Horror,
            amount: 1,
        }
    );
}

#[test]
fn deal_damage_adds_damage_and_emits_event() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = run(
        &mut cx,
        &dsl::deal_damage(InvestigatorTarget::You, 2u8),
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].damage(), 2);
    assert_event!(
        events,
        Event::DamageTaken { investigator, amount: 2 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn deal_horror_adds_horror_and_emits_event() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = run(
        &mut cx,
        &dsl::deal_horror(InvestigatorTarget::You, 1u8),
        EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 1);
    assert_event!(
        events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
    );
}

#[test]
fn deal_damage_at_max_health_defeats_investigator() {
    // Apply damage that exactly reaches max_health (8 from TEST_INV) via
    // Effect::Deal and assert the investigator is Defeated and
    // InvestigatorEliminated is emitted. Pre-load 5 accumulated_damage so
    // 5 + 3 = 8 = defeated with a 3-damage deal.
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.investigator_card.accumulated_damage = 5;
    let mut state = GameStateBuilder::new().with_investigator(inv).build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::deal_damage(InvestigatorTarget::You, 3u8),
        EvalContext::for_controller(id),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&id].status, Status::Defeated);
    assert_event!(
        events,
        Event::InvestigatorEliminated { investigator, .. } if *investigator == id
    );
}

#[test]
fn deal_amount_can_be_a_count_of_failure_margin() {
    // Build a Deal whose amount is the failure margin; fail-by 2 → 2 damage.
    let effect = dsl::deal_damage(
        InvestigatorTarget::You,
        IntExpr::Count(Quantity::SkillTestFailedBy),
    );
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let mut eval_ctx = EvalContext::for_controller(InvestigatorId(1));
    eval_ctx.set_failed_by(2);
    let outcome = run(&mut cx, &effect, eval_ctx);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&InvestigatorId(1)].damage(), 2);
    // Deal evaluates the IntExpr once and applies the result in a single hit;
    // fail-by 2 → amount 2 → one DamageTaken event with amount 2.
    assert_event!(events, Event::DamageTaken { investigator, amount: 2 } if *investigator == InvestigatorId(1));
}
