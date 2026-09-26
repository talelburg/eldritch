use super::*;
use crate::state::GameStateBuilder;
use crate::test_support;

#[test]
fn check_play_card_returns_err_for_unknown_hand_index() {
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_active_investigator(InvestigatorId(1))
        .build();
    let err = check_play_card(&state, InvestigatorId(1), 0).expect_err("empty hand should reject");
    assert!(
        err.contains("hand_index"),
        "error should mention hand_index, got: {err}"
    );
}

#[test]
fn check_play_card_returns_err_when_investigator_missing() {
    let state = GameStateBuilder::default().build();
    let err = check_play_card(&state, InvestigatorId(99), 0)
        .expect_err("missing investigator should reject");
    assert!(
        err.contains("not in state"),
        "error should say not in state, got: {err}"
    );
}

#[test]
fn payable_play_cost_reads_a_printed_number() {
    let code = CardCode::new("01018");
    assert_eq!(payable_play_cost(Some(0), &code), Ok(0));
    assert_eq!(payable_play_cost(Some(4), &code), Ok(4));
}

/// A `"–"` cost is not an unmodelled case: per the official FAQ, *"Cards
/// with a cost of '–' have no cost that can be paid, and therefore cannot
/// be played."* (`data/official-faq/Frequently_Asked_Questions.md`.) The
/// Necronomicon 01009 and every Dunwich permanent print it.
#[test]
fn payable_play_cost_rejects_a_dash_cost_permanently() {
    let err =
        payable_play_cost(None, &CardCode::new("01009")).expect_err("a \"–\" cost cannot be paid");
    assert!(
        err.contains("cannot be played"),
        "the reject should state the rule, not a deferral: {err}"
    );
    assert!(
        !err.contains("#501"),
        "a \"–\" cost is final behaviour, not deferred work: {err}"
    );
}

/// X ingests as `Some(-2)`, so without this arm `u8::try_from(-2)
/// .unwrap_or(0)` would make an X-cost card **free**. Jenny's Twin .45s
/// 02010 is in the compiled corpus.
#[test]
fn payable_play_cost_rejects_an_x_cost_as_unmodelled() {
    let err = payable_play_cost(Some(-2), &CardCode::new("02010"))
        .expect_err("an X cost is not yet modeled");
    assert!(
        err.contains("X cost") && err.contains("TODO(#577)"),
        "the reject should name X and the deferral: {err}"
    );
}
