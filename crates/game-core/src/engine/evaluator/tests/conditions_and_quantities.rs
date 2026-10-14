use super::*;
use crate::test_support::TEST_REGISTRY;

#[test]
fn location_has_clues_condition_tracks_clue_count() {
    let inv_id = InvestigatorId(1);
    let has_clues = Condition::Compare {
        quantity: Quantity::CluesAtControllerLocation,
        op: CmpOp::Gt,
        value: 0,
    };
    // Condition tracks clue presence at the controller's location.
    assert_eq!(
        eval_condition(
            &state_with_clues_at_location(1),
            &TEST_REGISTRY,
            &EvalContext::for_controller(inv_id),
            &has_clues
        ),
        Ok(true)
    );
    assert_eq!(
        eval_condition(
            &state_with_clues_at_location(0),
            &TEST_REGISTRY,
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
    assert_eq!(
        eval_quantity(
            &state_with_clues_at_location(2),
            &ctx,
            Quantity::CluesAtControllerLocation
        ),
        2
    );
    assert_eq!(
        eval_quantity(
            &state_with_clues_at_location(0),
            &ctx,
            Quantity::CluesAtControllerLocation
        ),
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
            &state_with_clues_at_location(2),
            &TEST_REGISTRY,
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
    assert!(eval_condition(&state_with_clues_at_location(1), &TEST_REGISTRY, &ctx, &has).unwrap());
    assert!(!eval_condition(&state_with_clues_at_location(0), &TEST_REGISTRY, &ctx, &has).unwrap());
}
