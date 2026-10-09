use super::*;

#[test]
fn end_turn_is_always_offered_at_the_open_turn() {
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .open_turn(InvestigatorId(1))
        .build();
    assert!(legal_actions(&state).contains(&TurnAction::EndTurn));
}

#[test]
fn no_actions_when_not_the_open_turn() {
    // No InvestigatorTurn frame on top (empty stack) → nothing to offer.
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .build();
    assert!(legal_actions(&state).is_empty());
}

#[test]
fn basic_actions_offered_with_a_revealed_location_and_an_action() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .open_turn(InvestigatorId(1))
        .build();
    // Place the investigator on a revealed location so Investigate is legal.
    let loc = test_support::test_location(10, "Study");
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state.locations.get_mut(&loc_id).unwrap().revealed = true;
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;

    let actions = legal_actions(&state);
    assert!(actions.contains(&TurnAction::Resource {
        investigator: InvestigatorId(1)
    }));
    assert!(actions.contains(&TurnAction::Draw {
        investigator: InvestigatorId(1)
    }));
    assert!(actions.contains(&TurnAction::Investigate {
        investigator: InvestigatorId(1)
    }));
}

#[test]
fn no_action_points_offers_only_end_turn() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .open_turn(InvestigatorId(1))
        .build();
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 0;
    // With 0 actions, only EndTurn (which needs no action point) is legal.
    assert_eq!(legal_actions(&state), vec![TurnAction::EndTurn]);
}

#[test]
fn investigate_absent_on_an_unrevealed_location() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .open_turn(InvestigatorId(1))
        .build();
    let mut loc = test_support::test_location(10, "Study");
    loc.revealed = false;
    let loc_id = loc.id;
    state.locations.insert(loc_id, loc);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(loc_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    assert!(!legal_actions(&state).contains(&TurnAction::Investigate {
        investigator: InvestigatorId(1)
    }));
}

#[test]
fn move_offers_one_option_per_connected_destination() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .open_turn(InvestigatorId(1))
        .build();
    let mut a = test_support::test_location(10, "A");
    let b = test_support::test_location(11, "B");
    a.connections = vec![b.id];
    let (a_id, b_id) = (a.id, b.id);
    state.locations.insert(a_id, a);
    state.locations.insert(b_id, b);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(a_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;

    let actions = legal_actions(&state);
    assert!(actions.contains(&TurnAction::Move {
        investigator: InvestigatorId(1),
        destination: b_id,
    }));
    // No self-move.
    assert!(!actions.contains(&TurnAction::Move {
        investigator: InvestigatorId(1),
        destination: a_id,
    }));
}

#[test]
fn move_absent_when_unaffordable() {
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .open_turn(InvestigatorId(1))
        .build();
    let mut a = test_support::test_location(10, "A");
    let b = test_support::test_location(11, "B");
    a.connections = vec![b.id];
    let (a_id, b_id) = (a.id, b.id);
    state.locations.insert(a_id, a);
    state.locations.insert(b_id, b);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(a_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 0;
    assert!(!legal_actions(&state)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { .. })));
}

#[test]
fn every_enumerated_action_is_accepted_by_its_handler() {
    // The cross-check that makes "defer routing" safe: each enumerated
    // action applies without Rejected (Done or AwaitingInput both mean
    // "accepted"). Uses the OptionId round-trip (the truest cross-check:
    // dispatch goes through `ResolveInput(PickSingle(OptionId))`, not the
    // typed arms). Apply to a fresh clone per action. The board has a
    // connected, revealed destination so a Move is enumerated and checked too.
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        // Investigate is applied below, and a skill test rejects on an empty
        // bag (a malformed-state guard the enumerator does not replicate).
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(InvestigatorId(1))
        .build();
    let mut a = test_support::test_location(10, "A");
    let b = test_support::test_location(11, "B");
    a.connections = vec![b.id];
    let (a_id, _b_id) = (a.id, b.id);
    state.locations.insert(a_id, a);
    state.locations.insert(b.id, b);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .current_location = Some(a_id);
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .actions_remaining = 3;
    // An enemy engaged with the active investigator → Fight + Evade enumerated.
    let mut foe = test_support::test_enemy(7, "Ghoul");
    foe.engaged_with = Some(InvestigatorId(1));
    foe.current_location = Some(a_id);
    state.enemies.insert(foe.id, foe);
    // A co-located unengaged enemy → Engage enumerated (its AoO comes from
    // the engaged foe above; that is enemy_attack, never a Rejected).
    let mut engageable = test_support::test_enemy(8, "Rat");
    engageable.current_location = Some(a_id);
    state.enemies.insert(engageable.id, engageable);
    // An advanceable act (threshold met) → AdvanceAct enumerated; a second
    // act so advancing is a clean transition, not a terminal resolution.
    state
        .investigators
        .get_mut(&InvestigatorId(1))
        .unwrap()
        .clues = 2;
    state.act_deck = vec![
        Act {
            code: CardCode("_act1".into()),
            clue_threshold: 2,
        },
        Act {
            code: CardCode("_act2".into()),
            clue_threshold: 99,
        },
    ];

    let actions = legal_actions(&state);
    for (i, action) in actions.iter().enumerate() {
        let result = engine::apply(
            state.clone(),
            Action::Player(PlayerAction::ResolveInput {
                response: InputResponse::PickSingle(OptionId(
                    u32::try_from(i).expect("action index fits u32"),
                )),
            }),
        );
        assert!(
            !matches!(result.outcome, EngineOutcome::Rejected { .. }),
            "enumerated {action:?} (OptionId {i}) rejected: {:?}",
            result.outcome,
        );
    }
}

#[test]
fn resolve_input_optionid_dispatches_enumerated_turn_action() {
    // EndTurn is always OptionId of its position in legal_actions; submitting it
    // via ResolveInput must dispatch (not reject) even while the open turn still
    // idles Done (pre-flip).
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .open_turn(InvestigatorId(1))
        .build();
    let actions = legal_actions(&state);
    let idx = actions
        .iter()
        .position(|a| *a == TurnAction::EndTurn)
        .expect("EndTurn offered");
    let result = engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(
                u32::try_from(idx).expect("action index fits u32"),
            )),
        }),
    );
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "open-turn OptionId dispatch rejected: {:?}",
        result.outcome
    );
}
