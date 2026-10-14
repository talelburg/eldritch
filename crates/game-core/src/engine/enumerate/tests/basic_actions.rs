use super::*;
use crate::test_support::TEST_REGISTRY;

/// The investigator's location.
const HERE: LocationId = LocationId(10);
/// The location connected to [`HERE`].
const NEXT_DOOR: LocationId = LocationId(11);

/// Investigator 1's open turn at [`HERE`] (location "A"), connected to
/// [`NEXT_DOOR`] ("B"), with the seat `edit` finishes.
fn open_turn_on_two_rooms(edit: impl FnOnce(Seat) -> Seat) -> GameStateBuilder {
    let mut a = test_support::test_location(HERE.0, "A");
    a.connections = vec![NEXT_DOOR];
    GameStateBuilder::default()
        .with_location(a)
        .with_location(test_support::test_location(NEXT_DOOR.0, "B"))
        .with_seat(edit(seat(1).at(HERE)))
        .open_turn(InvestigatorId(1))
}

#[test]
fn end_turn_is_always_offered_at_the_open_turn() {
    let state = GameStateBuilder::default()
        .with_seat(seat(1))
        .open_turn(InvestigatorId(1))
        .build();
    assert!(legal_actions(&state, &TEST_REGISTRY).contains(&TurnAction::EndTurn));
}

#[test]
fn no_actions_when_not_the_open_turn() {
    // No InvestigatorTurn frame on top (empty stack) → nothing to offer.
    let state = GameStateBuilder::default()
        .with_seat(seat(1))
        .with_phase(Phase::Investigation)
        .with_active_investigator(InvestigatorId(1))
        .build();
    assert!(legal_actions(&state, &TEST_REGISTRY).is_empty());
}

#[test]
fn basic_actions_offered_with_a_revealed_location_and_an_action() {
    // Place the investigator on a revealed location so Investigate is legal.
    let mut loc = test_support::test_location(HERE.0, "Study");
    loc.revealed = true;
    let state = GameStateBuilder::default()
        .with_location(loc)
        .with_seat(seat(1).at(HERE).actions(3))
        .open_turn(InvestigatorId(1))
        .build();

    let actions = legal_actions(&state, &TEST_REGISTRY);
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
    let state = GameStateBuilder::default()
        .with_seat(seat(1).actions(0))
        .open_turn(InvestigatorId(1))
        .build();
    // With 0 actions, only EndTurn (which needs no action point) is legal.
    assert_eq!(
        legal_actions(&state, &TEST_REGISTRY),
        vec![TurnAction::EndTurn]
    );
}

#[test]
fn investigate_absent_on_an_unrevealed_location() {
    let mut loc = test_support::test_location(HERE.0, "Study");
    loc.revealed = false;
    let state = GameStateBuilder::default()
        .with_location(loc)
        .with_seat(seat(1).at(HERE).actions(3))
        .open_turn(InvestigatorId(1))
        .build();
    assert!(
        !legal_actions(&state, &TEST_REGISTRY).contains(&TurnAction::Investigate {
            investigator: InvestigatorId(1)
        })
    );
}

#[test]
fn move_offers_one_option_per_connected_destination() {
    let state = open_turn_on_two_rooms(|s| s.actions(3)).build();

    let actions = legal_actions(&state, &TEST_REGISTRY);
    assert!(actions.contains(&TurnAction::Move {
        investigator: InvestigatorId(1),
        destination: NEXT_DOOR,
    }));
    // No self-move.
    assert!(!actions.contains(&TurnAction::Move {
        investigator: InvestigatorId(1),
        destination: HERE,
    }));
}

#[test]
fn move_absent_when_unaffordable() {
    let state = open_turn_on_two_rooms(|s| s.actions(0)).build();
    assert!(!legal_actions(&state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { .. })));
}

/// Exempt from the session idiom: the subject is the wire action itself.
/// A menu pick reaches the handler as `ResolveInput(PickSingle(OptionId))`
/// at the action's menu position, and these two tests send exactly that
/// rather than going through a step.
#[test]
fn every_enumerated_action_is_accepted_by_its_handler() {
    // The cross-check that makes "defer routing" safe: each enumerated
    // action applies without Rejected (Done or AwaitingInput both mean
    // "accepted"). Uses the OptionId round-trip (the truest cross-check:
    // dispatch goes through `ResolveInput(PickSingle(OptionId))`, not the
    // typed arms). Apply to a fresh clone per action. The board has a
    // connected, revealed destination so a Move is enumerated and checked too.
    // An enemy engaged with the active investigator → Fight + Evade enumerated.
    let mut foe = test_support::test_enemy(7, "Ghoul");
    foe.engaged_with = Some(InvestigatorId(1));
    foe.current_location = Some(HERE);
    // A co-located unengaged enemy → Engage enumerated (its AoO comes from
    // the engaged foe above; that is enemy_attack, never a Rejected).
    let mut engageable = test_support::test_enemy(8, "Rat");
    engageable.current_location = Some(HERE);
    // An advanceable act (threshold met) → AdvanceAct enumerated: the
    // investigator holds the 2 clues it needs.
    let mut state = open_turn_on_two_rooms(|s| s.actions(3).clues(2))
        // Investigate is applied below, and a skill test rejects on an empty
        // bag (a malformed-state guard the enumerator does not replicate).
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_enemy(foe)
        .with_enemy(engageable)
        .build();
    // No builder setter loads the act deck. A second act so advancing is a
    // clean transition, not a terminal resolution.
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
    let actions = legal_actions(&state, &TEST_REGISTRY);
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

/// Exempt from the session idiom, as
/// [`every_enumerated_action_is_accepted_by_its_handler`] is: the subject is
/// the `ResolveInput(PickSingle(OptionId))` wire action itself.
#[test]
fn resolve_input_optionid_dispatches_enumerated_turn_action() {
    // EndTurn is always OptionId of its position in legal_actions; submitting it
    // via ResolveInput must dispatch (not reject) even while the open turn still
    // idles Done (pre-flip).
    let state = GameStateBuilder::default()
        .with_seat(seat(1))
        .open_turn(InvestigatorId(1))
        .build();
    let actions = legal_actions(&state, &TEST_REGISTRY);
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
