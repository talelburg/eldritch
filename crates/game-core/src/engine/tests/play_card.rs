use super::*;

// Card actions can't use the non-enumeration form here: these states
// carry no `InvestigatorTurn` frame (so `legal_actions` returns empty
// and the assertion would be vacuous), and game-core's `TEST_INV`-only
// registry never enumerates a card-bearing PlayCard anyway. Instead we
// reach the handler directly via `dispatch_turn_action_unchecked`
// (bypassing the enumeration gate) and assert it rejects — testing the
// handler's defensive validation, exactly as the pre-#447 typed-action
// `apply(PlayCard)` tests did. (The *positive* PlayCard enumeration +
// the registry-backed flow live in `enumerate.rs` and
// `crates/cards/tests/play_card.rs`.)
fn play_card_state(active: bool, hand: Vec<CardCode>) -> (GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = hand;
    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv);
    if active {
        builder = builder.with_active_investigator(id);
    }
    (builder.build(), id)
}

#[test]
fn play_card_outside_investigation_phase_is_rejected() {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new("01059")];
    let state = GameStateBuilder::new()
        .with_phase(Phase::Mythos)
        .with_investigator(inv)
        .with_active_investigator(id)
        .build();
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}

#[test]
fn play_card_by_non_active_investigator_is_rejected() {
    let (state, id) = play_card_state(false, vec![CardCode::new("01059")]);
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}

#[test]
fn play_card_by_defeated_investigator_is_rejected() {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new("01059")];
    inv.status = Status::Defeated;
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .build();
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}

#[test]
fn play_card_with_out_of_bounds_hand_index_is_rejected() {
    let (state, id) = play_card_state(true, vec![CardCode::new("01059")]);
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 5,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}

#[test]
fn play_card_with_empty_hand_is_rejected() {
    let (state, id) = play_card_state(true, vec![]);
    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::PlayCard {
            investigator: id,
            hand_index: 0,
        },
    );
    assert!(
        matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "{:?}",
        result.outcome
    );
}
