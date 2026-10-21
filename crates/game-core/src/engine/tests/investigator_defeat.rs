use super::*;
use crate::test_support::{TEST_REGISTRIES, TEST_REGISTRY};

/// The enemy engaged with `INV` on a Move board.
const ENGAGED: EnemyId = EnemyId(200);

/// `mover`'s Move board with `enemy` engaged with them at the origin, taking
/// the Move: the session rests where play goes after it.
fn move_engaged_with(mover: Seat, enemy: Enemy) -> TestSession {
    move_board(mover)
        .with_enemy_engaged(enemy, INV)
        .session()
        .take(&MOVE_TO_CONNECTED)
}

#[test]
fn aoo_lethal_damage_defeats_investigator_during_move_and_cancels_move() {
    // `accumulated_damage` pre-loaded to 7, leaving 1 health remaining
    // (max_health() = 8 from TEST_INV): the enemy's 1-damage AoO is lethal.
    let result = move_engaged_with(seat(1).at(HERE).harm(7, 0), attacker(ENGAGED.0, 1, 0)).finish();

    // Single-investigator scenario: the defeat ends the game.
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Damage applied + defeat event fired.
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == INV
    );
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Damage,
        } if *investigator == INV
    );
    let state = &result.state;
    // Status flipped to Defeated.
    assert_eq!(state.investigators[&INV].status, Status::Defeated);
    // Action point still spent (the action declaration stays).
    assert_eq!(state.investigators[&INV].actions_remaining, 2);
    // Move suppressed: no InvestigatorMoved event; enemy stays at
    // the origin. The investigator's location is cleared to None
    // (elimination step 3 — they have left play).
    assert_no_event!(result.events, Event::InvestigatorMoved { .. });
    assert_eq!(
        state.investigators[&INV].current_location, None,
        "eliminated investigator has no location (left play)"
    );
    assert_eq!(state.enemies[&ENGAGED].current_location, Some(HERE));
    // Single-investigator scenario, so AllInvestigatorsEliminated
    // also fires.
    assert_event!(result.events, Event::AllInvestigatorsEliminated);
}

#[test]
fn aoo_lethal_horror_defeats_investigator_during_investigate_and_cancels_test() {
    // An Investigate board with an engaged enemy whose attack is pure
    // horror. Investigator has 1 sanity remaining (accumulated_horror=7;
    // max_sanity() = 8 from TEST_INV), so 1 horror drives them insane.
    let result = investigate_board(seat(1).at(HERE).harm(0, 7), 2, 2)
        .with_enemy_engaged(attacker(400, 0, 1), INV)
        .session()
        .take(&TurnAction::Investigate { investigator: INV })
        .finish();

    // The AoO defeats the only investigator before any commit window: the
    // game is over.
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV
    );
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Horror,
        } if *investigator == INV
    );
    assert_eq!(result.state.investigators[&INV].status, Status::Defeated);
    // Skill test suppressed: no SkillTestStarted event.
    assert_no_event!(result.events, Event::SkillTestStarted { .. });
    assert_no_event!(result.events, Event::CluePlaced { .. });
}

#[test]
fn aoo_damage_to_active_investigator_below_threshold_does_not_defeat() {
    // Sanity check: AoO that doesn't reach max_health leaves the
    // investigator Active. Same as the existing AoO test but
    // explicit on the status field and absence of defeat events.
    // max_health()=8 from TEST_INV registry; attack_damage=1 < 8 → survives.
    let session = move_engaged_with(seat(1).at(HERE), attacker(ENGAGED.0, 1, 0));
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].status, Status::Active);
    assert_no_event!(session.events(), Event::InvestigatorEliminated { .. });
    // Move proceeds.
    assert_event!(session.events(), Event::InvestigatorMoved { .. });
}

#[test]
fn defeated_investigator_does_not_take_further_damage() {
    // Two engaged ready enemies, both with attack_damage = 5.
    // Investigator has 1 health remaining (accumulated_damage=7). With 2
    // engaged, the Move's AoO suspends on the order pick; the
    // chosen first attacker defeats the investigator, so the active-check
    // at the loop top early-breaks before the second attacks (no re-prompt).
    // max_health()=8 from TEST_INV; 7 + 5 = 12 >= 8 = defeated.
    let r2 = move_board(seat(1).at(HERE).harm(7, 0))
        .with_enemy_engaged(attacker(ENGAGED.0, 5, 0), INV)
        .with_enemy_engaged(attacker(201, 5, 0), INV)
        .session()
        .take(&MOVE_TO_CONNECTED)
        .pick(OptionTarget::Enemy(ENGAGED))
        .finish();
    // The sole investigator was defeated, so Rules Reference p.10 step 6
    // applies — "If there are no remaining players, the scenario ends" —
    // and the engine's latch for it is `Lost`. The attack loop and the open
    // turn are therefore cancelled and the ending finalizes in this same
    // step, rather than the engine re-offering a turn menu to a table with
    // no remaining players.
    assert_eq!(r2.outcome, EngineOutcome::Done, "the scenario has ended");
    assert_event!(r2.events, Event::ScenarioResolved { .. });
    // The subject includes a stack invariant: no frame survives the ending.
    assert!(
        r2.state.continuations.is_empty(),
        "no stranded frames after the ending: {:?}",
        r2.state.continuations,
    );
    // Exactly one DamageTaken event (from the chosen first AoO) and one
    // InvestigatorEliminated; the second enemy never attacks (early-break).
    assert_event_count!(r2.events, 1, Event::DamageTaken { .. });
    assert_event_count!(r2.events, 1, Event::InvestigatorEliminated { .. });
    // Only one AoO fired: pre-loaded 7 + first AoO's 5 = 12; if the
    // second AoO had fired too, damage() would be 17 instead.
    assert_eq!(r2.state.investigators[&INV].damage(), 12);
    // The actor was defeated mid-action → the Move's primary effect is
    // suppressed by the re-validation gate.
    assert_no_event!(r2.events, Event::InvestigatorMoved { .. });
}

#[test]
fn aoo_with_lethal_damage_and_sublethal_horror_applies_both_numerically() {
    // Rules Reference page 7: damage and horror from a single
    // attack are applied simultaneously. Lethal damage MUST NOT
    // short-circuit the horror application.
    // Pre-load accumulated_damage so remaining health = 5 (lethal with attack_damage=5).
    // max_health()=8 from TEST_INV; 3 + 5 = 8 = defeated.
    // Sanity headroom: max_sanity()=8, attack_horror=1 → 0+1=1 < 8 → sub-lethal.
    let result = move_engaged_with(seat(1).at(HERE).harm(3, 0), attacker(ENGAGED.0, 5, 1)).finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Both events fire and both numeric fields land.
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 5 } if *investigator == INV
    );
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV
    );
    // Pre-loaded 3 + attack 5 = 8; horror 0 + 1 = 1.
    assert_eq!(result.state.investigators[&INV].damage(), 8);
    assert_eq!(result.state.investigators[&INV].horror(), 1);
    // Exactly one InvestigatorEliminated, caused by Damage.
    assert_event_count!(result.events, 1, Event::InvestigatorEliminated { .. });
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Damage,
        } if *investigator == INV
    );
    assert_eq!(result.state.investigators[&INV].status, Status::Defeated);
}

#[test]
fn aoo_with_sublethal_damage_and_lethal_horror_applies_both_numerically() {
    // Symmetric to the lethal-damage case: lethal horror MUST NOT
    // short-circuit the damage application.
    // accumulated_horror 3: remaining sanity = 8 - 3 = 5, and 3 + 5 = 8 =
    // defeated; damage 1 < 8 is sub-lethal.
    let result = move_engaged_with(seat(1).at(HERE).harm(0, 3), attacker(ENGAGED.0, 1, 5)).finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == INV
    );
    assert_event!(
        result.events,
        Event::HorrorTaken { investigator, amount: 5 } if *investigator == INV
    );
    // damage: 0 + 1 = 1; horror: pre-loaded 3 + attack 5 = 8.
    assert_eq!(result.state.investigators[&INV].damage(), 1);
    assert_eq!(result.state.investigators[&INV].horror(), 8);
    assert_event_count!(result.events, 1, Event::InvestigatorEliminated { .. });
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Horror,
        } if *investigator == INV
    );
    assert_eq!(result.state.investigators[&INV].status, Status::Defeated);
}

#[test]
fn aoo_with_both_lethal_defeats_once_with_damage_cause() {
    // Both stats cross their threshold from the same attack. Per
    // the enemy_attack doc comment, the tie-break is
    // EliminationCause::Damage (Rules Reference is silent on the
    // simultaneous-lethal case; damage-first is the convention).
    // Pre-load both counters so remaining health and sanity = 1 each.
    // max_health()=max_sanity()=8 from TEST_INV; 7+1=8 = defeated for both.
    let result = move_engaged_with(seat(1).at(HERE).harm(7, 7), attacker(ENGAGED.0, 1, 1)).finish();
    assert_eq!(result.outcome, EngineOutcome::Done);
    // Pre-loaded 7 + attack 1 = 8 each.
    assert_eq!(result.state.investigators[&INV].damage(), 8);
    assert_eq!(result.state.investigators[&INV].horror(), 8);
    assert_event_count!(result.events, 1, Event::InvestigatorEliminated { .. });
    assert_event!(
        result.events,
        Event::InvestigatorEliminated {
            investigator,
            cause: EliminationCause::Damage,
        } if *investigator == INV
    );
    assert_eq!(result.state.investigators[&INV].status, Status::Defeated);
}

#[test]
fn all_investigators_eliminated_fires_only_when_last_active_falls() {
    // Two investigators, one defeated, then the second defeated.
    // AllInvestigatorsEliminated should fire only on the second.
    let inv2 = InvestigatorId(2);
    // Pre-load accumulated_damage so remaining health = 1 (lethal with attack_damage=1).
    // max_health()=8 from TEST_INV; 7+1=8=defeated. inv2 stays at default 8/8.
    let session = move_board(seat(1).at(HERE).harm(7, 0))
        .with_seat(seat(2))
        .with_chaos_bag(bag_only_zero())
        .with_turn_order([INV, inv2])
        .with_enemy_engaged(attacker(500, 1, 0), INV)
        .session();

    // inv1 moves → AoO defeats them. inv2 is still Active, and play goes on
    // to their turn.
    let session = session.take(&MOVE_TO_CONNECTED);
    session.assert_at_turn_menu(inv2);
    assert_event!(
        session.events(),
        Event::InvestigatorEliminated { investigator, .. } if *investigator == INV
    );
    assert_no_event!(session.events(), Event::AllInvestigatorsEliminated);
    assert_eq!(session.state().investigators[&INV].status, Status::Defeated);
    assert_eq!(session.state().investigators[&inv2].status, Status::Active);
}

/// `board`'s built state with `INV` defeated (no seat setter carries status).
fn with_inv_defeated(board: GameStateBuilder) -> GameState {
    let mut state = board.build();
    state.investigators.get_mut(&INV).unwrap().status = Status::Defeated;
    state
}

#[test]
fn defeated_investigator_cannot_move() {
    let state = with_inv_defeated(
        move_board(seat(1).at(HERE)).with_enemy_engaged(attacker(ENGAGED.0, 1, 0), INV),
    );
    // Defeated status → Move is not legal.
    assert!(!legal_actions(&state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Move { investigator, destination } if *investigator == INV && *destination == CONNECTED)));
}

#[test]
fn defeated_investigator_cannot_investigate() {
    let state = with_inv_defeated(investigate_board(seat(1).at(HERE), 2, 2));
    // Defeated status → Investigate is not legal.
    assert!(!legal_actions(&state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Investigate { investigator } if *investigator == INV)));
}

#[test]
fn defeated_investigator_cannot_fight() {
    let state = with_inv_defeated(fight_evade_board(seat(1).at(HERE), ghoul()));
    // Defeated status → Fight is not legal.
    assert!(!legal_actions(&state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Fight { investigator, enemy } if *investigator == INV && *enemy == GHOUL)));
}

#[test]
fn defeated_investigator_cannot_evade() {
    let state = with_inv_defeated(fight_evade_board(seat(1).at(HERE), ghoul()));
    // Defeated status → Evade is not legal.
    assert!(!legal_actions(&state, &TEST_REGISTRY)
        .iter()
        .any(|a| matches!(a, TurnAction::Evade { investigator, enemy } if *investigator == INV && *enemy == GHOUL)));
}

#[test]
fn defeated_investigator_cannot_perform_skill_test() {
    // Investigator 2's turn is open; a card effect would have the defeated
    // investigator 1 test.
    let inv2 = InvestigatorId(2);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .with_seat(seat(2))
        .with_turn_order([INV, inv2])
        .with_chaos_bag(bag_only_zero())
        .open_turn(inv2)
        .build();
    state.investigators.get_mut(&INV).unwrap().status = Status::Defeated;
    let session = TestSession::new(state, TEST_REGISTRIES).test_skill(INV, SkillKind::Willpower, 0);
    session.expect_rejected();
    session.assert_at_turn_menu(inv2);
    assert!(session.finish().events.is_empty());
}
