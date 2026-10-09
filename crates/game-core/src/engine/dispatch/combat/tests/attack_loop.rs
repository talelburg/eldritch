use super::*;
use crate::state::EnemyPhaseFrame;

/// The parked loop's re-exposure (#704): the head attacker's sequence has
/// popped, so the loop takes it off, exhausts it, and — with none left —
/// runs the enemy phase's cursor advance.
///
/// One attacker, not two: with 2+ remaining the drain would re-prompt for
/// the player attack order (#143), which the order-pick tests cover. The
/// test registry (`TEST_INV`) is installed so `max_health()`/`max_sanity()`
/// resolve (#448 cp2a).
#[test]
fn drive_parked_attack_loop_exhausts_the_head_then_advances_the_cursor() {
    let inv_id = InvestigatorId(1);
    let attacker = EnemyId(2);
    let mut enemy = test_support::test_enemy(2, "Attacker");
    enemy.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    state.continuations.push(AttackLoopFrame {
        investigator: inv_id,
        remaining_attackers: vec![attacker], // the head, mid-sequence
        source: EnemyAttackSource::EnemyPhase,
        stage: AttackLoopStage::Attacking,
    });

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = drive_parked_attack_loop(&mut cx);

    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::AttackLoop(_))),
        "the parked attack-loop frame is consumed"
    );
    assert!(
        state.enemies[&attacker].exhausted,
        "the head attacker exhausts once its sequence completes"
    );
    assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == attacker);
    // Loop finished → `after_enemy_phase_attacks` advanced the cursor past
    // the only investigator and opened the all-attacked window (auto-skips
    // inline with no ability to offer), cascading the EnemyPhase anchor off
    // the stack — so no anchor is left still attacking anyone.
    assert!(
        !state.continuations.iter().any(|c| matches!(
            c,
            Continuation::EnemyPhase(EnemyPhaseFrame {
                attacking: Some(_),
                ..
            })
        )),
        "cursor advanced past the sole investigator (no anchor still attacking)"
    );
    let _ = outcome;
}

/// The exhaust is the *enemy phase's* step, not the attack's impact, so it
/// runs whether or not the attack dealt anything — `data/arkhamdb-faq/core/01023.md`:
/// *"If an attack was cancelled during the Enemy phase, the attacking enemy
/// still exhausts."* A cancelled attack reaches here having dealt no damage
/// (the coordinator abandoned its sequence before the resolve step), which
/// this fixture reproduces by parking the loop with no damage applied. The
/// end-to-end cancel is `crates/cards/tests/dodge.rs`.
#[test]
fn a_head_attacker_that_dealt_nothing_still_exhausts() {
    let inv_id = InvestigatorId(1);
    let attacker = EnemyId(2);
    let mut enemy = test_support::test_enemy(2, "Attacker");
    enemy.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    state.continuations.push(AttackLoopFrame {
        investigator: inv_id,
        remaining_attackers: vec![attacker],
        source: EnemyAttackSource::EnemyPhase,
        stage: AttackLoopStage::Attacking,
    });

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let _ = drive_parked_attack_loop(&mut cx);

    assert_eq!(
        state.investigators[&inv_id].damage(),
        0,
        "no damage: the attack's impact never ran"
    );
    assert_no_event!(events, Event::DamageTaken { .. });
    assert!(
        state.enemies[&attacker].exhausted,
        "a cancelled attack still exhausts the attacker (01023 ruling)"
    );
}

/// The mirror of the rule above on the other two sources: an `AoO` or
/// retaliate attacker never exhausts (RR p.7 / p.18), cancelled or not.
#[test]
fn an_attack_of_opportunity_attacker_never_exhausts() {
    let inv_id = InvestigatorId(1);
    let attacker = EnemyId(2);
    let mut enemy = test_support::test_enemy(2, "Attacker");
    enemy.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        .build();
    state.continuations.push(AttackLoopFrame {
        investigator: inv_id,
        remaining_attackers: vec![attacker],
        source: EnemyAttackSource::AttackOfOpportunity,
        stage: AttackLoopStage::Attacking,
    });

    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let _ = drive_parked_attack_loop(&mut cx);

    assert!(
        !state.enemies[&attacker].exhausted,
        "an AoO attacker never exhausts (RR p.7)"
    );
    assert_no_event!(events, Event::EnemyExhausted { .. });
}

#[test]
fn drive_retaliate_deals_damage_but_does_not_exhaust_the_attacker() {
    // RR p.18: a retaliate attack does not exhaust the attacker.
    let inv_id = InvestigatorId(1);
    let mut enemy = test_support::test_enemy(100, "Retaliator");
    enemy.retaliate = true;
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    // Not engaged: a retaliate fires regardless of engagement, driven by enemy id.
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    // The attack is queued on the coordinator (#704), so drive it out: the
    // loop's `Done` means *queued*, not *dealt*.
    let outcome = drive_retaliate(&mut cx, EnemyId(100), inv_id);
    let outcome = dispatch::drive(&mut cx, outcome);

    assert!(matches!(outcome, EngineOutcome::Done));
    assert!(
        !cx.state.enemies[&EnemyId(100)].exhausted,
        "retaliate must not exhaust (RR p.18)"
    );
    assert_eq!(
        cx.state.investigators[&inv_id].damage(),
        1,
        "retaliate dealt 1 damage"
    );
    assert_event!(events, Event::DamageTaken { .. });
    assert_no_event!(events, Event::EnemyExhausted { .. });
}

#[test]
fn drive_aoo_deals_damage_but_does_not_exhaust_the_attacker() {
    // RR p.7: an enemy does not exhaust while making an attack of opportunity.
    let inv_id = InvestigatorId(1);
    let mut enemy = test_support::test_enemy(100, "Ghoul");
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .build();
    let mut events = Vec::new();
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };

    let outcome = drive_aoo(&mut cx, inv_id);
    let outcome = dispatch::drive(&mut cx, outcome);

    assert!(matches!(outcome, EngineOutcome::Done));
    assert!(
        !cx.state.enemies[&EnemyId(100)].exhausted,
        "AoO must not exhaust the attacker (RR p.7)"
    );
    assert_eq!(
        cx.state.investigators[&inv_id].damage(),
        1,
        "AoO damage landed on the investigator"
    );
    // Enemy attack fires DamageTaken (no EnemyAttacked event exists); verify
    // damage landed on the investigator and no exhaust event was emitted.
    assert_event!(events, Event::DamageTaken { .. });
    assert_no_event!(events, Event::EnemyExhausted { .. });
}

#[test]
fn drive_aoo_offers_order_pick_for_two_engaged_enemies() {
    // 2 engaged ready enemies provoking an AoO → the loop suspends on the
    // order pick, parking the AttackLoop frame as the top frame with the AoO
    // source + PickOrder stage (so it spans the whole AoO, #143). Picking the
    // higher-id enemy first proves the pick overrides EnemyId order; neither
    // AoO attacker exhausts (RR p.7). Total AoO damage = 3 < 8 = TEST_INV's
    // max_health() (#448 cp2a).
    let inv_id = InvestigatorId(1);
    let mut e_a = test_support::test_enemy(5, "A"); // EnemyId(5), dmg 1
    e_a.engaged_with = Some(inv_id);
    e_a.attack_damage = 1;
    let mut e_b = test_support::test_enemy(6, "B"); // EnemyId(6), dmg 2
    e_b.engaged_with = Some(inv_id);
    e_b.attack_damage = 2;

    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(e_a)
        .with_enemy(e_b)
        .build();
    let mut events = Vec::new();

    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let outcome = drive_aoo(&mut cx, inv_id);
    let outcome = dispatch::drive(&mut cx, outcome);
    assert!(
        matches!(outcome, EngineOutcome::AwaitingInput { .. }),
        "2 engaged ready enemies → AoO order pick (#143)"
    );
    // The parked frame carries the AoO source + PickOrder stage (frame spans
    // the whole AoO, not just a window suspension).
    assert!(matches!(
        state.continuations.top(),
        Some(Continuation::AttackLoop(AttackLoopFrame {
            source: EnemyAttackSource::AttackOfOpportunity,
            stage: AttackLoopStage::PickOrder,
            ..
        }))
    ));

    // Pick EnemyId(6) (dmg 2) first → option 1 in EnemyId order [5, 6].
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let resumed = dispatch::resolve_input(&mut cx, &InputResponse::PickSingle(OptionId(1)));
    let resumed = dispatch::drive(&mut cx, resumed);
    assert!(matches!(resumed, EngineOutcome::Done), "AoO loop drained");
    let damages: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::DamageTaken { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(
        damages,
        vec![2, 1],
        "chosen EnemyId(6) (dmg 2) struck first"
    );
    assert!(
        !state.enemies[&EnemyId(5)].exhausted && !state.enemies[&EnemyId(6)].exhausted,
        "AoO attackers never exhaust (RR p.7)"
    );
}

#[test]
fn resume_attack_order_pick_rejects_invalid_input_and_keeps_frame() {
    // An out-of-range option id and a wrong InputResponse variant both reject,
    // leaving the PickOrder frame on the stack for the client to retry
    // (mirrors resume_hunter_choice).
    let inv_id = InvestigatorId(1);
    let mut e_a = test_support::test_enemy(5, "A");
    e_a.engaged_with = Some(inv_id);
    let mut e_b = test_support::test_enemy(6, "B");
    e_b.engaged_with = Some(inv_id);

    let mut state = GameStateBuilder::new()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(e_a)
        .with_enemy(e_b)
        .build();
    let mut events = Vec::new();
    let _ = drive_aoo(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        inv_id,
    );

    // Out-of-range option (only 0, 1 valid).
    let rejected = dispatch::resolve_input(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &InputResponse::PickSingle(OptionId(9)),
    );
    assert!(matches!(rejected, EngineOutcome::Rejected { .. }));
    // Wrong variant.
    let rejected2 = dispatch::resolve_input(
        &mut Cx {
            state: &mut state,
            events: &mut events,
        },
        &InputResponse::Skip,
    );
    assert!(matches!(rejected2, EngineOutcome::Rejected { .. }));
    // The PickOrder frame survives both rejections for retry.
    assert!(matches!(
        state.continuations.top(),
        Some(Continuation::AttackLoop(AttackLoopFrame {
            stage: AttackLoopStage::PickOrder,
            ..
        }))
    ));
}
