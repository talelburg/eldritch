use super::*;

/// Build a `GameState` with `clue_count` clues at `InvestigatorId(1)`'s location.
fn with_clues(clue_count: u8) -> GameState {
    let loc_id = LocationId(1);
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(loc_id);
    let mut loc = test_support::test_location(1, "Study");
    loc.clues = clue_count;
    GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc)
        .build()
}

#[test]
fn location_has_clues_condition_tracks_clue_count() {
    let inv_id = InvestigatorId(1);
    let loc_id = LocationId(1);
    let with_clues_local = |clue_count: u8| {
        let mut inv = test_support::test_investigator(1);
        inv.current_location = Some(loc_id);
        let mut loc = test_support::test_location(1, "Study");
        loc.clues = clue_count;
        GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build()
    };
    let has_clues = Condition::Compare {
        quantity: Quantity::CluesAtControllerLocation,
        op: CmpOp::Gt,
        value: 0,
    };
    // Condition tracks clue presence at the controller's location.
    assert_eq!(
        eval_condition(
            &with_clues_local(1),
            &EvalContext::for_controller(inv_id),
            &has_clues
        ),
        Ok(true)
    );
    assert_eq!(
        eval_condition(
            &with_clues_local(0),
            &EvalContext::for_controller(inv_id),
            &has_clues
        ),
        Ok(false)
    );
}

#[test]
fn eval_quantity_reads_clues_engaged_and_margin() {
    // clues at location
    let (state, inv) = state_with_cards_in_play(&[]);
    let ctx = EvalContext::for_controller(inv);
    // helper `with_clues(n)` already exists in this module; reuse it:
    assert_eq!(
        eval_quantity(&with_clues(2), &ctx, Quantity::CluesAtControllerLocation),
        2
    );
    assert_eq!(
        eval_quantity(&with_clues(0), &ctx, Quantity::CluesAtControllerLocation),
        0
    );
    // failure margin from the ctx binding
    let mut ctx2 = EvalContext::for_controller(inv);
    ctx2.set_failed_by(3);
    assert_eq!(eval_quantity(&state, &ctx2, Quantity::SkillTestFailedBy), 3);
    assert_eq!(eval_quantity(&state, &ctx, Quantity::SkillTestFailedBy), 0);
}

#[test]
fn eval_count_and_compare_over_clues() {
    let (_s, inv) = state_with_cards_in_play(&[]);
    let ctx = EvalContext::for_controller(inv);
    // Count
    assert_eq!(
        eval_int_expr(
            &with_clues(2),
            &ctx,
            &IntExpr::Count(Quantity::CluesAtControllerLocation)
        )
        .unwrap(),
        2
    );
    // Compare: clues > 0
    let has = Condition::Compare {
        quantity: Quantity::CluesAtControllerLocation,
        op: CmpOp::Gt,
        value: 0,
    };
    assert!(eval_condition(&with_clues(1), &ctx, &has).unwrap());
    assert!(!eval_condition(&with_clues(0), &ctx, &has).unwrap());
}
