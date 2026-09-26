use super::*;

#[test]
fn chosen_investigator_single_candidate_auto_binds() {
    // 1 investigator ⇒ auto-bind, no input.
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let before = state.investigators[&id].resources;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::chosen_anywhere(), 2),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(state.investigators[&id].resources, before + 2);
    assert!(state.continuations.is_empty());
}

#[test]
fn chosen_investigator_two_candidates_suspends_then_binds_the_pick() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .build();
    let before1 = state.investigators[&InvestigatorId(1)].resources;
    let before2 = state.investigators[&InvestigatorId(2)].resources;
    let mut events = Vec::new();
    // Two candidates ⇒ suspend.
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::chosen_anywhere(), 5),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        state.investigators[&InvestigatorId(1)].resources,
        before1,
        "suspend mutates nothing",
    );

    // Resume with pick = option 1 → the second investigator (BTreeMap
    // sorted order) gains.
    let outcome = resume_pick(&mut state, &mut events, 1);
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(2)].resources,
        before2 + 5
    );
    assert_eq!(state.investigators[&InvestigatorId(1)].resources, before1);
}

#[test]
fn chosen_location_two_candidates_suspends() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(test_support::test_location(1, "A"))
        .with_location(test_support::test_location(2, "B"))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::discover_clue(LocationTarget::chosen_anywhere(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(offered_count(&outcome), 2, "two locations offered");
    assert_suspended_leaf(&state);
}

#[test]
fn chosen_location_here_auto_binds_the_controllers_location() {
    // Two locations present, but `Here` filters to the controller's own ⇒
    // singleton ⇒ auto-bind (no Choice frame), unlike `Anywhere` which
    // would offer both and suspend.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(test_support::test_location(1, "A"))
        .with_location(test_support::test_location(2, "B"))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::discover_clue(
            LocationTarget::Chosen(Choose {
                scope: LocationSet::Here,
            }),
            1,
        ),
        ctx(1),
    );
    assert!(
        !matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "Here is a singleton ⇒ auto-binds, never suspends: {outcome:?}",
    );
    assert!(
        state.continuations.is_empty(),
        "no Choice frame for a singleton scope",
    );
}

#[test]
fn chosen_at_your_location_auto_binds_the_sole_co_located_investigator() {
    // Investigator 1 (controller) and 2 are in play; only 1 is at the
    // controller's location. `At(Here)` must offer only investigator 1 and
    // auto-bind it (1 candidate ⇒ no suspend) — `Anywhere` would see 2 and
    // suspend.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_location(test_support::test_location(1, "A"))
        .with_location(test_support::test_location(2, "B"))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .current_location = Some(LocationId(2));
    let before1 = state.investigators[&InvestigatorId(1)].resources;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::chosen_at_your_location(), 2),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].resources,
        before1 + 2
    );
    assert!(
        state.continuations.is_empty(),
        "single co-located candidate auto-binds"
    );
}

#[test]
fn chosen_at_your_location_suspends_when_two_are_co_located() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_location(test_support::test_location(1, "A"))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        offered_count(&outcome),
        2,
        "two co-located investigators offered"
    );
    assert_suspended_leaf(&state);
}

#[test]
fn chosen_at_your_location_rejects_when_controller_between_locations() {
    // test_investigator defaults to current_location = None.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .build();
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::gain_resources(InvestigatorTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(state.continuations.is_empty());
}

#[test]
fn deal_damage_to_chosen_enemy_at_your_location_auto_binds_and_damages() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(test_support::test_location(1, "A"))
        .with_location(test_support::test_location(2, "B"))
        .with_enemy({
            let mut e = test_support::test_enemy(100, "Ghoul");
            e.max_health = 3;
            e.current_location = Some(LocationId(1));
            e
        })
        .with_enemy({
            let mut e = test_support::test_enemy(101, "Faraway");
            e.max_health = 3;
            e.current_location = Some(LocationId(2));
            e
        })
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.enemies[&EnemyId(100)].damage,
        1,
        "co-located enemy damaged"
    );
    assert_eq!(
        state.enemies[&EnemyId(101)].damage,
        0,
        "faraway enemy untouched"
    );
    assert!(
        state.continuations.is_empty(),
        "sole co-located candidate auto-binds"
    );
}

#[test]
fn deal_damage_to_chosen_enemy_suspends_when_two_are_co_located() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(test_support::test_location(1, "A"))
        .with_enemy({
            let mut e = test_support::test_enemy(100, "G1");
            e.current_location = Some(LocationId(1));
            e
        })
        .with_enemy({
            let mut e = test_support::test_enemy(101, "G2");
            e.current_location = Some(LocationId(1));
            e
        })
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(offered_count(&outcome), 2, "two co-located enemies offered");
    assert_suspended_leaf(&state);
}

#[test]
fn deal_damage_to_chosen_enemy_rejects_when_none_co_located() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_location(test_support::test_location(1, "A"))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    assert!(state.continuations.is_empty());
}

#[test]
fn heal_target_chosen_at_your_location_auto_binds() {
    test_support::install_test_registry();
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_location(test_support::test_location(1, "A"))
        .with_location(test_support::test_location(2, "B"))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .current_location = Some(LocationId(2));
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .investigator_card
        .accumulated_damage = 2;
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::heal(
            HarmKind::Damage,
            InvestigatorTarget::chosen_at_your_location(),
            1,
        ),
        ctx(1),
    );
    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        state.investigators[&InvestigatorId(1)].damage(),
        1,
        "sole co-located target healed"
    );
    assert!(state.continuations.is_empty());
}

#[test]
fn heal_target_chosen_suspends_when_two_are_co_located() {
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_location(test_support::test_location(1, "A"))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(LocationId(1));
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .unwrap()
        .current_location = Some(LocationId(1));
    // Both carry damage: RR "Target" (#639) makes an investigator with
    // nothing to heal an ineligible target, so a suspend needs two
    // *eligible* candidates, not merely two co-located ones.
    for id in [InvestigatorId(1), InvestigatorId(2)] {
        state
            .investigators
            .get_mut(&id)
            .unwrap()
            .investigator_card
            .accumulated_damage = 2;
    }
    let mut events = Vec::new();
    let outcome = run(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &dsl::heal(
            HarmKind::Damage,
            InvestigatorTarget::chosen_at_your_location(),
            1,
        ),
        ctx(1),
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        offered_count(&outcome),
        2,
        "two co-located heal targets offered"
    );
    assert_suspended_leaf(&state);
}

#[test]
fn grounded_choice_anchors_enemy_options() {
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    let cands = [EnemyId(4), EnemyId(9)];
    let out = resolve_grounded_choice(
        ctx,
        &cands,
        "empty",
        "Choose an enemy",
        |id| format!("{id:?}"),
        |id| Some(OptionTarget::Enemy(*id)),
        |_id| ctx,
        false, // 2 candidates → suspend regardless of the flag
    );
    match out {
        Err(EngineOutcome::AwaitingInput { request, .. }) => {
            assert_eq!(
                request.options[0].target,
                Some(OptionTarget::Enemy(EnemyId(4)))
            );
            assert_eq!(
                request.options[1].target,
                Some(OptionTarget::Enemy(EnemyId(9)))
            );
        }
        other => panic!("2 candidates suspend for a pick, got {other:?}"),
    }
}

#[test]
fn grounded_choice_investigator_stays_unanchored() {
    let ctx = EvalContext::for_controller(InvestigatorId(1));
    let cands = [InvestigatorId(1), InvestigatorId(2)];
    let out = resolve_grounded_choice(
        ctx,
        &cands,
        "empty",
        "Choose an investigator",
        |id| format!("{id:?}"),
        |_id| None, // out of scope for S5
        |_id| ctx,
        false,
    );
    match out {
        Err(EngineOutcome::AwaitingInput { request, .. }) => {
            assert!(request.options.iter().all(|o| o.target.is_none()));
        }
        other => panic!("2 candidates suspend, got {other:?}"),
    }
}
