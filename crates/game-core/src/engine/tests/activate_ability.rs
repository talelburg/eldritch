use super::*;
use crate::state::Owner;

// As with PlayCard (`play_card.rs`), the non-enumeration form is vacuous
// here (no `InvestigatorTurn` frame; `TEST_INV`-only registry yields no
// abilities for "01059"). Reach the handler directly via
// `dispatch_turn_action_unchecked` and assert it rejects — the handler's
// defensive validation, exactly as the pre-#447 typed-action tests did.
// The registry-backed activation flow lives in
// crates/game-core/tests/activate_ability.rs.
fn activate_ability_state(active: bool) -> (GameState, InvestigatorId, CardInstanceId) {
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(7);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01059"),
        instance_id,
        Owner::Investigator(InvestigatorId(1)),
    ));
    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv);
    if active {
        builder = builder.with_active_investigator(id);
    }
    (builder.build(), id, instance_id)
}

#[test]
fn activate_ability_outside_investigation_phase_is_rejected() {
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01059"),
        instance_id,
        Owner::Investigator(InvestigatorId(1)),
    ));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator(inv)
        .with_active_investigator(id)
        .build();
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}

#[test]
fn activate_ability_by_non_active_investigator_is_rejected() {
    let (state, id, instance_id) = activate_ability_state(false);
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}

#[test]
fn activate_ability_with_unknown_instance_id_is_rejected() {
    let (state, id, _real_instance) = activate_ability_state(true);
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(CardInstanceId(9999)),
            address: AbilityAddress::Printed(0),
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}

#[test]
fn activate_ability_when_defeated_is_rejected() {
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_support::test_investigator(1);
    inv.status = Status::Defeated;
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("01059"),
        instance_id,
        Owner::Investigator(InvestigatorId(1)),
    ));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .build();
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}
