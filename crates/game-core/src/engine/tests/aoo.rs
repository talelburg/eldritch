use super::*;
use crate::test_support::TEST_REGISTRIES;

/// `INV`'s open turn at the Study with a one-card deck (so a draw succeeds
/// without the empty-deck horror path muddying an `AoO` assertion).
fn draw_board() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).deck(["_test_card_1"]))
        .open_turn(INV)
}

#[test]
fn move_with_ready_engaged_enemy_fires_aoo_and_enemy_follows() {
    let enemy_id = EnemyId(200);
    let session = move_board(seat(1).at(HERE))
        .with_enemy_engaged(attacker(200, 1, 0), INV)
        .session()
        .take(&MOVE_TO_CONNECTED);

    session.assert_at_turn_menu(INV);
    let events = session.events();
    assert_event!(
        events,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == INV
    );
    // AoO damage must fire BEFORE the move resolves per the Rules
    // Reference. assert_event! is existence-only, so check the
    // positions explicitly.
    let damage_idx = events
        .iter()
        .position(|e| matches!(e, Event::DamageTaken { .. }))
        .expect("DamageTaken event missing");
    let moved_idx = events
        .iter()
        .position(|e| matches!(e, Event::InvestigatorMoved { .. }))
        .expect("InvestigatorMoved event missing");
    assert!(
        damage_idx < moved_idx,
        "AoO DamageTaken (idx {damage_idx}) must precede InvestigatorMoved (idx {moved_idx})"
    );
    let state = session.state();
    // Investigator damaged.
    assert_eq!(state.investigators[&INV].damage(), 1);
    // Investigator moved.
    assert_eq!(state.investigators[&INV].current_location, Some(CONNECTED));
    assert_event!(
        events,
        Event::InvestigatorMoved { from, to, .. } if *from == HERE && *to == CONNECTED
    );
    // Engaged enemy followed.
    assert_eq!(state.enemies[&enemy_id].current_location, Some(CONNECTED));
    assert_eq!(state.enemies[&enemy_id].engaged_with, Some(INV));
    // AoO does NOT exhaust per the Rules Reference.
    assert!(!state.enemies[&enemy_id].exhausted);
}

#[test]
fn move_with_exhausted_engaged_enemy_does_not_fire_aoo() {
    let enemy_id = EnemyId(200);
    let mut enemy = attacker(200, 1, 0);
    enemy.exhausted = true;
    let session = move_board(seat(1).at(HERE))
        .with_enemy_engaged(enemy, INV)
        .session()
        .take(&MOVE_TO_CONNECTED);

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_no_event!(session.events(), Event::HorrorTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
    // Exhausted enemy still follows the investigator.
    assert_eq!(
        session.state().enemies[&enemy_id].current_location,
        Some(CONNECTED)
    );
}

#[test]
fn draw_action_fires_aoo_from_ready_engaged_enemy() {
    let session = draw_board()
        .with_enemy_engaged(attacker(200, 1, 0), INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].damage(), 1);
    assert_event!(
        session.events(),
        Event::DamageTaken { investigator, amount: 1 } if *investigator == INV
    );
}

#[test]
fn draw_with_lethal_aoo_suppresses_the_draw() {
    // A lethal AoO (attack_damage == max_health) defeats the investigator
    // before the card is drawn; the draw is suppressed (no CardsDrawn event)
    // while the AoO damage still lands. Action is still spent.
    let result = draw_board()
        // 8 == test_investigator max_health.
        .with_enemy_engaged(attacker(200, 8, 0), INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV })
        .finish();

    // Lethal AoO landed, and with the only investigator defeated the game is
    // over.
    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::DamageTaken { investigator, amount: 8 } if *investigator == INV
    );
    // Action was spent before the lethal AoO (3 → 2).
    assert_event!(
        result.events,
        Event::ActionsRemainingChanged { investigator, new_count: 2 }
            if *investigator == INV
    );
    // ...but the draw was suppressed.
    assert_no_event!(result.events, Event::CardsDrawn { .. });
}

#[test]
fn draw_with_no_engaged_enemy_draws_normally() {
    // Behaviour-preserving: no AoO enemy, so the draw resolves without
    // interruption. One card drawn, back at the turn menu.
    let session = draw_board().session();
    let hand_before = session.state().investigators[&INV].hand.len();

    let session = session.take(&TurnAction::Draw { investigator: INV });

    session.assert_at_turn_menu(INV);
    assert_eq!(
        session.state().investigators[&INV].hand.len(),
        hand_before + 1,
        "one card drawn"
    );
    assert_event!(
        session.events(),
        Event::CardsDrawn { investigator, count: 1 } if *investigator == INV
    );
    assert_no_event!(session.events(), Event::DamageTaken { .. });
}

#[test]
fn move_with_unengaged_enemy_at_origin_leaves_enemy_behind() {
    // An enemy at A, not engaged with anyone, so the move provokes no AoO,
    // keeping the focus on the unengaged enemy.
    let other_id = EnemyId(201);
    let mut other = test_support::test_enemy(201, "Bystander");
    other.current_location = Some(HERE);
    let session = move_board(seat(1).at(HERE))
        .with_enemy(other)
        .session()
        .take(&MOVE_TO_CONNECTED);

    session.assert_at_turn_menu(INV);
    // Investigator moved.
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(CONNECTED)
    );
    // Unengaged enemy stayed put.
    assert_eq!(
        session.state().enemies[&other_id].current_location,
        Some(HERE)
    );
}

#[test]
fn investigate_with_ready_engaged_enemy_fires_aoo() {
    // An Investigate board with an engaged enemy at the investigator's
    // location.
    let session = take_committing_nothing(
        investigate_board(seat(1).at(HERE), 2, 2).with_enemy_engaged(attacker(300, 0, 1), INV),
        &TurnAction::Investigate { investigator: INV },
    );

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV
    );
    // Skill test still runs after AoO.
    assert_event!(session.events(), Event::SkillTestStarted { .. });
    assert_eq!(session.state().investigators[&INV].horror(), 1);
}

#[test]
fn fight_does_not_fire_aoo_from_other_engaged_enemy() {
    // Investigator engaged with the Fight target AND a second
    // ready engaged enemy. Fight is on the AoO-exempt list, so
    // no AoO fires — neither from the target nor from the
    // bystander.
    let session = take_committing_nothing(
        fight_evade_board(seat(1).at(HERE), ghoul()).with_enemy_engaged(attacker(202, 5, 0), INV),
        &TurnAction::Fight {
            investigator: INV,
            enemy: GHOUL,
        },
    );

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_no_event!(session.events(), Event::HorrorTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}

#[test]
fn evade_does_not_fire_aoo_from_other_engaged_enemy() {
    let session = take_committing_nothing(
        fight_evade_board(seat(1).at(HERE), ghoul()).with_enemy_engaged(attacker(203, 5, 0), INV),
        &TurnAction::Evade {
            investigator: INV,
            enemy: GHOUL,
        },
    );

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_no_event!(session.events(), Event::HorrorTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
}

#[test]
fn move_with_no_engaged_enemy_does_not_fire_aoo() {
    // Regression: the AoO step is a no-op when no engaged
    // enemies exist; pre-existing Move tests should not have
    // started failing.
    let session = move_board(seat(1).at(HERE))
        .session()
        .take(&MOVE_TO_CONNECTED);
    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::DamageTaken { .. });
}

#[test]
fn aoo_order_pick_resolves_attacks_in_chosen_order_for_multiple_attackers() {
    // 3 engaged ready enemies provoke an AoO on Move; with 2+ attackers the
    // player picks the order one at a time (#143, RR p.25 step 3.3). Pick the
    // highest-damage enemy first to prove the pick overrides EnemyId order.
    // TEST_INV has 8 health; total enemy damage = 1+2+4 = 7 < 8, so investigator survives.
    // (max_health is now read from the registry, not a field — see #448 cp4.)
    let board = [(300, 1), (301, 2), (302, 4)]
        .into_iter()
        .fold(move_board(seat(1).at(HERE)), |board, (id, dmg)| {
            board.with_enemy_engaged(attacker(id, dmg, 0), INV)
        });
    // Move provokes the AoO; 3 engaged → order pick (no attack dealt yet).
    // Pick EnemyId(302) (dmg 4) first → resolves it, then re-prompts over the
    // remaining [300, 301]; pick EnemyId(301) (dmg 2), and EnemyId(300)
    // (dmg 1) is then forced. The AoO loop drains and the parked Move
    // completes.
    let session = board
        .session()
        .take(&MOVE_TO_CONNECTED)
        .pick(OptionTarget::Enemy(EnemyId(302)))
        .pick(OptionTarget::Enemy(EnemyId(301)));
    // Back at the turn menu once the AoO loop drains.
    session.assert_at_turn_menu(INV);

    let damages: Vec<u8> = session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::DamageTaken { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(
        damages,
        vec![4, 2, 1],
        "chosen order: 302 (dmg4), 301 (dmg2), 300 (dmg1)"
    );
    assert_eq!(session.state().investigators[&INV].damage(), 7);
    // The Move completed once the AoO loop drained.
    assert_event!(session.events(), Event::InvestigatorMoved { .. });
}

#[test]
fn aoo_from_zero_damage_zero_horror_enemy_emits_no_events() {
    // Edge: an engaged ready enemy with attack_damage = 0 and
    // attack_horror = 0 still "attacks" but the helper's `if > 0`
    // guards must skip both event emissions.
    let session = move_board(seat(1).at(HERE))
        .with_enemy_engaged(attacker(310, 0, 0), INV)
        .session()
        .take(&MOVE_TO_CONNECTED);
    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_no_event!(session.events(), Event::HorrorTaken { .. });
    assert_eq!(session.state().investigators[&INV].damage(), 0);
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}
