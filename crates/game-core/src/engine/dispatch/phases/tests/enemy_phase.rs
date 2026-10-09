use super::*;
use crate::action::{Action, InputResponse, PlayerAction};
use crate::engine::outcome::{EngineOutcome, OptionId, OptionTarget};
use crate::engine::{self, dispatch};
use crate::state::{
    EnemyId, FastActorScope, GameStateBuilder, InvestigatorId, LocationId, Phase, Status,
};
use crate::{assert_event, test_support};

#[test]
fn enemy_phase_runs_hunters_then_attack_loop_when_no_tie() {
    let mut loc_a = test_support::test_location(1, "A");
    let mut loc_b = test_support::test_location(2, "B");
    loc_a.connections = vec![LocationId(2)];
    loc_b.connections = vec![LocationId(1)];
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(2));
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_location(loc_a)
        .with_location(loc_b)
        .with_investigator(inv)
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .build();
    let mut events = Vec::new();
    let outcome = {
        // end_turn may push the next phase's Entry anchor (slice 1b); drive
        // completes the transition, as the apply boundary does in production.
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let o = end_turn(&mut cx);
        dispatch::drive(&mut cx, o)
    };
    // No registry installed → the attack window auto-skips inline and the
    // cascade runs Enemy→Upkeep→Mythos within this same call, pausing at the
    // step-1.4 encounter-draw prompt (AwaitingInput). The hunter still moved
    // + engaged during step 3.2 — asserted via the event stream below.
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert_event!(events, Event::EnemyEngaged { enemy, .. } if *enemy == EnemyId(1));
}

#[test]
fn enemy_phase_suspends_on_hunter_tie_then_resumes_into_attack_loop() {
    let mut loc_a = test_support::test_location(1, "A");
    let mut loc_b = test_support::test_location(2, "B");
    let mut loc_c = test_support::test_location(3, "C");
    let mut loc_d = test_support::test_location(4, "D");
    loc_a.connections = vec![LocationId(2), LocationId(3)];
    loc_b.connections = vec![LocationId(1), LocationId(4)];
    loc_c.connections = vec![LocationId(1), LocationId(4)];
    loc_d.connections = vec![LocationId(2), LocationId(3)];
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(4));
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_investigator(inv)
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .build();
    let mut events = Vec::new();
    let outcome = {
        // end_turn may push the next phase's Entry anchor (slice 1b); drive
        // completes the transition, as the apply boundary does in production.
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let o = end_turn(&mut cx);
        dispatch::drive(&mut cx, o)
    };
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(state.phase, Phase::Enemy);
    // The EnemyPhase anchor (slice 1a) is on the stack beneath the
    // suspended hunter-movement choice.
    assert!(
        state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EnemyPhase(_))),
        "EnemyPhase anchor present while suspended in the Enemy phase; stack = {:?}",
        state.continuations,
    );
    let mut ev2 = Vec::new();
    // Pick LocationId(2) by the anchor its option carries.
    let EngineOutcome::AwaitingInput { request, .. } = &outcome else {
        unreachable!("asserted AwaitingInput above");
    };
    // Hand-built pick: this test drives the dispatch entry points on a `Cx`
    // directly, below the apply boundary a `TestSession` steps through.
    let pick = request
        .options
        .iter()
        .find(|o| o.target == Some(OptionTarget::Location(LocationId(2))))
        .expect("LocationId(2) among offered options")
        .id;
    let resumed = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut ev2,
        };
        let o = dispatch::resolve_input(&mut cx, &InputResponse::PickSingle(pick));
        dispatch::drive(&mut cx, o) // slice 1b: complete the cascade
    };
    // With no registry the attack window auto-skips and the cascade runs
    // Enemy->Upkeep->Mythos within the same resume call, pausing at the
    // step-1.4 encounter-draw prompt (AwaitingInput).
    assert!(matches!(resumed, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(state.phase, Phase::Mythos);
}

#[test]
fn resolve_attacks_for_investigator_fires_engaged_ready_enemy_and_exhausts() {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(1);
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    enemy.attack_horror = 0;
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([inv_id])
        .with_enemy(enemy)
        // The loop's own tail advances the enemy-phase cursor once it drains
        // (#704), so it needs its anchor; driving past it cascades on into
        // the next phase, which these assertions do not read.
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    let mut events = Vec::new();

    // The attack is queued on the timing coordinator (#704), so drive it out.
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let queued = combat::resolve_attacks_for_investigator(&mut cx, inv_id);
    let _ = dispatch::drive(&mut cx, queued);

    // Damage placed.
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )),
        "expected DamageTaken {{ amount: 1 }}; events = {events:?}"
    );

    // Exhaust asserted on the event, not on post-drive state: driving the
    // drained loop's tail cascades on into Upkeep, which readies every
    // exhausted enemy again. The event is the record of the moment.
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )),
        "expected EnemyExhausted; events = {events:?}"
    );

    // Ordering: DamageTaken precedes EnemyExhausted (post-attack exhaust).
    let damage_pos = events
        .iter()
        .position(|e| matches!(e, Event::DamageTaken { .. }))
        .unwrap();
    let exhaust_pos = events
        .iter()
        .position(|e| matches!(e, Event::EnemyExhausted { .. }))
        .unwrap();
    assert!(
        damage_pos < exhaust_pos,
        "DamageTaken must precede EnemyExhausted; events = {events:?}"
    );
}

#[test]
fn resolve_attacks_for_investigator_excludes_exhausted_and_unengaged_enemies() {
    let inv_id = InvestigatorId(1);

    // Engaged but exhausted — must NOT attack.
    let mut e1 = test_support::test_enemy(1, "Exhausted Engaged");
    e1.engaged_with = Some(inv_id);
    e1.exhausted = true;
    e1.attack_damage = 5;

    // Ready but unengaged — must NOT attack.
    let mut e2 = test_support::test_enemy(2, "Ready Unengaged");
    e2.engaged_with = None;
    e2.attack_damage = 5;

    // Ready engaged — the only one that attacks.
    let mut e3 = test_support::test_enemy(3, "Ready Engaged");
    e3.engaged_with = Some(inv_id);
    e3.attack_damage = 1;

    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_turn_order([inv_id])
        .with_enemy(e1)
        .with_enemy(e2)
        .with_enemy(e3)
        // See the sibling test: the drained loop advances its own cursor.
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    let mut events = Vec::new();

    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let queued = combat::resolve_attacks_for_investigator(&mut cx, inv_id);
    let _ = dispatch::drive(&mut cx, queued);

    // Exactly one DamageTaken (from e3, amount 1).
    let damages: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::DamageTaken { .. }))
        .collect();
    assert_eq!(
        damages.len(),
        1,
        "exactly one attacker should fire; events = {events:?}"
    );
    assert!(matches!(damages[0], Event::DamageTaken { amount: 1, .. }));

    // Only e3 exhausted (e1 was already, and does not re-emit; e2 never
    // attacked). Asserted on events rather than post-drive state — the
    // cascade past the drained loop reaches Upkeep, which readies enemies.
    let exhausted_events: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::EnemyExhausted { .. }))
        .collect();
    assert_eq!(exhausted_events.len(), 1);
    assert!(matches!(
        exhausted_events[0],
        Event::EnemyExhausted { enemy: EnemyId(3) }
    ));
}

#[test]
fn resolve_attacks_for_investigator_pick_overrides_enemy_id_order() {
    let inv_id = InvestigatorId(1);

    let mut e_lower = test_support::test_enemy(2, "Lower id"); // EnemyId(2), dmg 1
    e_lower.engaged_with = Some(inv_id);
    e_lower.attack_damage = 1;

    let mut e_higher = test_support::test_enemy(10, "Higher id"); // EnemyId(10), dmg 2
    e_higher.engaged_with = Some(inv_id);
    e_higher.attack_damage = 2;

    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1)) // TEST_INV: 8 health; 1+2=3 total damage < 8
        .with_turn_order([inv_id])
        .with_enemy(e_higher) // inserted non-id order: BTreeMap still snapshots 2 then 10
        .with_enemy(e_lower)
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    let mut events = Vec::new();

    // 2 ready engaged enemies → suspend on the order pick (#143), not EnemyId order.
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let queued = combat::resolve_attacks_for_investigator(&mut cx, inv_id);
    let outcome = dispatch::drive(&mut cx, queued);
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected an attack-order prompt, got {outcome:?}");
    };
    // Options are the snapshotted attackers in EnemyId order: option 0 =
    // EnemyId(2), option 1 = EnemyId(10). Pick the higher-id enemy (dmg 2) to
    // strike FIRST, proving the player's pick overrides the deterministic order.
    // Hand-built pick: this test drives the dispatch entry points on a `Cx`
    // directly, below the apply boundary a `TestSession` steps through.
    let pick = request
        .options
        .iter()
        .find(|o| o.target == Some(OptionTarget::Enemy(EnemyId(10))))
        .expect("EnemyId(10) offered")
        .id;
    assert_eq!(
        pick,
        OptionId(1),
        "EnemyId(10) is option 1 in EnemyId order"
    );

    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let resumed = dispatch::resolve_input(&mut cx, &InputResponse::PickSingle(pick));
    // Driving past the drained loop cascades into the next phase, so the
    // outcome here is that phase's prompt rather than the loop's; what this
    // test pins is the order the two attacks landed in.
    let _ = dispatch::drive(&mut cx, resumed);
    // Both attacks resolved; the chosen (EnemyId 10, dmg 2) struck first.
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
        "chosen EnemyId(10) (dmg 2) attacked before EnemyId(2) (dmg 1)"
    );
    let exhausted: Vec<EnemyId> = events
        .iter()
        .filter_map(|e| match e {
            Event::EnemyExhausted { enemy } => Some(*enemy),
            _ => None,
        })
        .collect();
    assert_eq!(
        exhausted,
        vec![EnemyId(10), EnemyId(2)],
        "each attacker exhausts as its own attack completes"
    );
}

#[test]
fn resolve_attacks_for_investigator_early_breaks_when_target_defeated_mid_loop() {
    let inv_id = InvestigatorId(1);

    // EnemyId(1) deals the killing blow on its attack.
    let mut e1 = test_support::test_enemy(1, "Killer");
    e1.engaged_with = Some(inv_id);
    e1.attack_damage = 1;

    // EnemyId(2) must NOT attack (active check fails at loop top).
    let mut e2 = test_support::test_enemy(2, "Bystander");
    e2.engaged_with = Some(inv_id);
    e2.attack_damage = 5;

    let mut state = GameStateBuilder::default()
        .with_investigator({
            let mut inv = test_support::test_investigator(1);
            // Pre-load accumulated_damage so remaining health = 1 (lethal with attack_damage=1).
            // max_health()=8 from TEST_INV; 7+1=8=defeated.
            inv.investigator_card.accumulated_damage = 7;
            inv
        })
        .with_turn_order([inv_id])
        .with_enemy(e1)
        .with_enemy(e2)
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .build();
    let mut events = Vec::new();

    // 2 engaged → order pick first (#143). Pick EnemyId(1) (the killer) to
    // strike first; after it defeats the investigator, the active check at the
    // loop top early-breaks before any re-prompt, so EnemyId(2) never attacks.
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let queued = combat::resolve_attacks_for_investigator(&mut cx, inv_id);
    let outcome = dispatch::drive(&mut cx, queued);
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected an order pick, got {outcome:?}");
    };
    // Hand-built pick: this test drives the dispatch entry points on a `Cx`
    // directly, below the apply boundary a `TestSession` steps through.
    let pick = request
        .options
        .iter()
        .find(|o| o.target == Some(OptionTarget::Enemy(EnemyId(1))))
        .expect("EnemyId(1) offered")
        .id;
    let mut cx = Cx {
        state: &mut state,
        events: &mut events,
    };
    let resumed = dispatch::resolve_input(&mut cx, &InputResponse::PickSingle(pick));
    let _ = dispatch::drive(&mut cx, resumed);

    // e1's attack killed the sole investigator, so the scenario's resolution
    // latched and the parked attack loop is one of the frames that cancels
    // (ADR 0004 — `cancelled_by_scenario_end`). Nothing further in the round
    // happens, including e1's own exhaust, which since #704 follows its
    // attack rather than being part of it.
    assert!(
        state.ending.is_some(),
        "the sole investigator's defeat ended the scenario"
    );
    assert!(
        !state.enemies[&EnemyId(1)].exhausted && !state.enemies[&EnemyId(2)].exhausted,
        "the ended scenario cancels the rest of the loop"
    );

    let damages: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::DamageTaken { .. }))
        .collect();
    assert_eq!(
        damages.len(),
        1,
        "only e1's attack lands; events = {events:?}"
    );

    // No exhaust at all: e1's attack ended the scenario before its own
    // post-attack exhaust step ran (see the assertions above), and e2 never
    // attacked.
    crate::assert_no_event!(events, Event::EnemyExhausted { .. });

    // Investigator was defeated.
    assert_eq!(state.investigators[&inv_id].status, Status::Defeated);
}

#[test]
fn enemy_phase_emits_phase_started_and_cascades_to_mythos_in_no_eligibility_case() {
    // 1 Active investigator, no engaged enemies. Auto-skip
    // cascades through both windows + enemy_phase_end +
    // Upkeep → Mythos.
    let inv_id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Investigation)
        .build();
    state.turn_order = vec![inv_id];
    state.active_investigator = None;
    let mut events = Vec::new();

    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Investigation → Enemy

    // Positional ordering of the major events.
    let pos = |pred: &dyn Fn(&Event) -> bool| events.iter().position(pred);
    let started = pos(&|e| {
        matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Enemy
            }
        )
    })
    .expect("PhaseStarted(Enemy)");
    let ended = pos(&|e| {
        matches!(
            e,
            Event::PhaseEnded {
                phase: Phase::Enemy
            }
        )
    })
    .expect("PhaseEnded(Enemy)");
    let upkeep_started = pos(&|e| {
        matches!(
            e,
            Event::PhaseStarted {
                phase: Phase::Upkeep
            }
        )
    })
    .expect("PhaseStarted(Upkeep)");

    assert!(
        started < ended && ended < upkeep_started,
        "ordered: 3.1 → 3.4 → Upkeep 4.1; events = {events:?}"
    );
    assert_eq!(state.phase, Phase::Mythos, "cascade lands in Mythos");
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EnemyPhase(_))),
        "EnemyPhase anchor popped at phase end (cursor gone with it)"
    );
}

#[test]
fn enemy_phase_with_two_investigators_iterates_in_turn_order() {
    // Each investigator is engaged with a ready enemy; the per-investigator
    // attack step must fire for both, in turn order — observable as a
    // DamageTaken per investigator (id1 before id2).
    let id1 = InvestigatorId(1);
    let id2 = InvestigatorId(2);
    let mut e1 = test_support::test_enemy(1, "Enemy 1");
    e1.engaged_with = Some(id1);
    e1.attack_damage = 1;
    let mut e2 = test_support::test_enemy(2, "Enemy 2");
    e2.engaged_with = Some(id2);
    e2.attack_damage = 1;
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_enemy(e1)
        .with_enemy(e2)
        .with_phase(Phase::Investigation)
        .build();
    state.turn_order = vec![id1, id2];
    state.active_investigator = None;
    let mut events = Vec::new();

    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Investigation → Enemy

    // One per-investigator attack landed for each, in turn order.
    let dmg1 = events
        .iter()
        .position(|e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id1))
        .expect("id1 attacked");
    let dmg2 = events
        .iter()
        .position(|e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id2))
        .expect("id2 attacked");
    assert!(
        dmg1 < dmg2,
        "investigators attacked in turn order (id1 before id2); events = {events:?}"
    );
}

#[test]
fn enemy_phase_skips_eliminated_investigator_in_advance() {
    // All three investigators are engaged with a ready enemy, but id2 is
    // Defeated (eliminated). The per-investigator attack step must skip id2 —
    // observable as DamageTaken for id1 and id3 only, none for id2.
    let id1 = InvestigatorId(1);
    let id2 = InvestigatorId(2);
    let id3 = InvestigatorId(3);
    let mut e1 = test_support::test_enemy(1, "Enemy 1");
    e1.engaged_with = Some(id1);
    e1.attack_damage = 1;
    let mut e2 = test_support::test_enemy(2, "Enemy 2");
    e2.engaged_with = Some(id2);
    e2.attack_damage = 1;
    let mut e3 = test_support::test_enemy(3, "Enemy 3");
    e3.engaged_with = Some(id3);
    e3.attack_damage = 1;
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_investigator(test_support::test_investigator(2))
        .with_investigator(test_support::test_investigator(3))
        .with_enemy(e1)
        .with_enemy(e2)
        .with_enemy(e3)
        .with_phase(Phase::Investigation)
        .build();
    state.turn_order = vec![id1, id2, id3];
    state.active_investigator = None;
    state.investigators.get_mut(&id2).unwrap().status = Status::Defeated;
    let mut events = Vec::new();

    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Investigation → Enemy

    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id1)),
        "id1 attacked"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id3)),
        "id3 attacked"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::DamageTaken { investigator, .. } if *investigator == id2)),
        "Defeated id2 must be skipped (no attack against it); events = {events:?}"
    );
}

#[test]
fn enemy_phase_with_all_eliminated_opens_after_all_directly() {
    let id1 = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Investigation)
        .build();
    state.turn_order = vec![id1];
    state.active_investigator = None;
    state.investigators.get_mut(&id1).unwrap().status = Status::Defeated;
    let mut events = Vec::new();

    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Investigation → Enemy

    // With all investigators eliminated, no per-investigator attack step
    // runs (nobody to attack) and the cascade keeps going:
    // Enemy → Upkeep (no-op steps for empty Active set) → Mythos
    // (mythos_draw_pending = None → auto-skip path) → Investigation.
    // With no investigators left to attack there is no per-investigator
    // effect to observe; the cascade landing in Investigation is the
    // structural signal that the Enemy phase ran to completion.
    assert_eq!(state.phase, Phase::Investigation);
}

#[test]
fn enemy_phase_attack_lands_in_full_cascade() {
    // 1 investigator engaged with 1 ready enemy. Full Investigation→Enemy→Upkeep→Mythos
    // cascade; attack lands inside the BeforeInvestigatorAttacked continuation.
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(1);
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .with_phase(Phase::Investigation)
        .build();
    state.turn_order = vec![inv_id];
    state.active_investigator = None;
    let mut events = Vec::new();

    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Investigation → Enemy

    // The attack landed. Event-stream evidence — state.enemies's
    // `exhausted` flag is reset by Upkeep step 4.3 later in the
    // cascade (ready_exhausted_cards), so checking the post-cascade
    // state directly would race the readying step. The
    // DamageTaken + EnemyExhausted events emitted inside the
    // BeforeInvestigatorAttacked continuation are the authoritative
    // signal that the attack landed.
    assert!(events.iter().any(|e| matches!(
        e,
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::EnemyExhausted { enemy } if *enemy == enemy_id
    )));

    // Cascade landed in Mythos.
    assert_eq!(state.phase, Phase::Mythos);
}

#[test]
fn step_phase_from_enemy_does_not_emit_phase_ended_enemy() {
    // Direct unit-level check: step_phase emits no PhaseEnded itself,
    // so the Enemy→Upkeep step must not emit PhaseEnded(Enemy)
    // (enemy_phase_end owns that emit).
    let mut state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_phase(Phase::Enemy)
        .build();
    state.turn_order = vec![InvestigatorId(1)];
    state.active_investigator = None;
    // Use a state where Upkeep's cascade can complete (Active investigator exists).
    let mut events = Vec::new();

    step_phase(&mut Cx {
        state: &mut state,
        events: &mut events,
    }); // Enemy → Upkeep

    // step_phase itself MUST NOT emit PhaseEnded(Enemy); only
    // enemy_phase_end is allowed to (which doesn't run here — we
    // started in Enemy and stepped out, simulating the "phase
    // transition without driver-owned end emit" path).
    let phase_ended_enemy_count = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::PhaseEnded {
                    phase: Phase::Enemy
                }
            )
        })
        .count();
    assert_eq!(
        phase_ended_enemy_count, 0,
        "step_phase must NOT emit PhaseEnded(Enemy); only enemy_phase_end may. events = {events:?}"
    );
}

#[test]
fn enemy_phase_resumes_via_skip_input() {
    // Construct the state mid-pause: a BeforeInvestigatorAttacked
    // window is on the stack with empty pending_triggers (the
    // "pure-Fast window" shape that open_fast_window pushes when
    // Fast play is eligible), and the cursor points at inv1.
    //
    // Submitting PlayerAction::ResolveInput(InputResponse::Skip)
    // routes through resolve_input's "open_windows non-empty +
    // no reaction triggers" branch → close_reaction_window →
    // anchor_on_child_pop's BeforeInvestigatorAttacked arm →
    // resolve_attacks_for_investigator → cursor advance to None →
    // open AfterAllInvestigatorsAttacked → auto-skip continuation
    // → enemy_phase_end → cascade Upkeep → Mythos.
    //
    // The synthetic empty window (staged via with_open_window) fakes the
    // pause point because a real Fast-eligibility setup would require either
    // a card-registry install (heavyweight integration test) or a Fast
    // event card in hand with resources — neither tractable in
    // the engine layer. The Skip path itself is the load-bearing
    // resume mechanism this test exercises.
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(1);
    let mut enemy = test_support::test_enemy(1, "Test Enemy");
    enemy.engaged_with = Some(inv_id);
    enemy.attack_damage = 1;
    // active_investigator defaults to None. The EnemyPhase anchor (slice 1a)
    // sits beneath the synthetic BeforeInvestigatorAttacked window staged
    // above it; the window's close routes to anchor_on_child_pop.
    let state = GameStateBuilder::default()
        .with_investigator(test_support::test_investigator(1))
        .with_enemy(enemy)
        .with_phase(Phase::Enemy)
        .with_turn_order([inv_id])
        .with_phase_anchor(EnemyPhaseFrame {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: Some(inv_id),
        })
        .with_open_window(
            FastWindowKind::Phase(PhaseStep::BeforeInvestigatorAttacked),
            FastActorScope::Any,
        )
        .build();

    let result = engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::Skip,
        }),
    );

    // The Skip resumes the continuation; the cascade runs into Mythos and
    // pauses at the step-1.4 encounter-draw prompt (AwaitingInput).
    match result.outcome {
        EngineOutcome::AwaitingInput { .. } => {}
        ref other => panic!(
            "expected AwaitingInput (Mythos draw prompt) after Skip; got {other:?}; events = {:?}",
            result.events
        ),
    }
    assert_eq!(
        result.state.phase,
        Phase::Mythos,
        "cascade lands in Mythos after Skip resumed the continuation"
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
        )),
        "attack should have landed during the resumed continuation; events = {:?}",
        result.events
    );
    assert!(
        result.events.iter().any(|e| matches!(
            e,
            Event::EnemyExhausted { enemy } if *enemy == enemy_id
        )),
        "EnemyExhausted should fire during the resumed continuation; events = {:?}",
        result.events
    );
    assert!(
        !result
            .state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EnemyPhase(_))),
        "the EnemyPhase anchor (with its `attacking` cursor) is gone after the \
         continuation advances past the last Active investigator and the AfterAll \
         window auto-skips"
    );
}

// The companion pause-on-Fast-eligibility path is covered at
// `crates/cards/tests/enemy_phase_fast_window.rs` (#842): making anything
// Fast-eligible needs `cards::REGISTRY` installed, which this crate cannot
// reach by crate-dependency direction.
