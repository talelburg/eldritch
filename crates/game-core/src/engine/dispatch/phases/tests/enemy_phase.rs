use super::*;
use crate::engine::enumerate::TurnAction;
use crate::engine::outcome::{EngineOutcome, OptionTarget};
use crate::state::{
    Enemy, EnemyId, FastActorScope, GameStateBuilder, InvestigatorId, LocationId, Phase, Status,
};
use crate::test_support::{self, Seat};
use crate::{assert_event, assert_event_count, assert_event_sequence, assert_no_event};

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Investigator 1's open turn at [`HERE`], engaged by `attackers`. Taking
/// `EndTurn` ends the Investigation phase, so the Enemy phase's attack step
/// (3.3) runs against them.
fn attacked_on_end_turn(investigator: Seat, attackers: Vec<Enemy>) -> GameStateBuilder {
    attackers.into_iter().fold(
        GameStateBuilder::new()
            .with_location(test_support::test_location(HERE.0, "Study"))
            // A filler deck so Upkeep 4.4's draw doesn't deal deck-out horror.
            .with_seat(investigator.at(HERE).deck(["filler0"]))
            .open_turn(INV),
        |builder, enemy| builder.with_enemy_engaged(enemy, INV),
    )
}

/// A ready enemy dealing `damage` damage and no horror.
fn attacker(id: u32, name: &str, damage: u8) -> Enemy {
    let mut enemy = test_support::test_enemy(id, name);
    enemy.attack_damage = damage;
    enemy.attack_horror = 0;
    enemy
}

#[test]
fn enemy_phase_runs_hunters_then_attack_loop_when_no_tie() {
    let mut loc_a = test_support::test_location(1, "A");
    let mut loc_b = test_support::test_location(2, "B");
    loc_a.connections = vec![LocationId(2)];
    loc_b.connections = vec![LocationId(1)];
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(LocationId(1));
    let session = GameStateBuilder::new()
        .with_location(loc_a)
        .with_location(loc_b)
        .with_seat(test_support::seat(1).at(LocationId(2)))
        .with_enemy(hunter)
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn);
    // No ability in the test registry → the attack window auto-skips and the
    // cascade runs Enemy→Upkeep→Mythos within this same step, pausing at the
    // step-1.4 encounter-draw prompt. The hunter still moved + engaged during
    // step 3.2 — asserted via the event stream below.
    session.assert_at_encounter_draw();
    let state = session.state();
    assert_eq!(state.phase, Phase::Mythos);
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert_event!(session.events(), Event::EnemyEngaged { enemy, .. } if *enemy == EnemyId(1));
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
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(LocationId(1));
    let session = GameStateBuilder::new()
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_seat(test_support::seat(1).at(LocationId(4)))
        .with_enemy(hunter)
        .open_turn(INV)
        .session()
        .take(&TurnAction::EndTurn);
    // The hunter's two equidistant routes are the lead's choice.
    assert_eq!(
        session.offered_at(&OptionTarget::Location(LocationId(2))),
        1
    );
    assert_eq!(session.state().phase, Phase::Enemy);
    // The EnemyPhase anchor (slice 1a) is on the stack beneath the
    // suspended hunter-movement choice — a stack invariant, so read the stack.
    assert!(
        session
            .state()
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EnemyPhase(_))),
        "EnemyPhase anchor present while suspended in the Enemy phase; stack = {:?}",
        session.state().continuations,
    );
    let session = session.pick(OptionTarget::Location(LocationId(2)));
    // With no ability to offer the attack window auto-skips and the cascade runs
    // Enemy->Upkeep->Mythos within the same step, pausing at the step-1.4
    // encounter-draw prompt.
    session.assert_at_encounter_draw();
    assert_eq!(session.state().phase, Phase::Mythos);
}
#[test]
fn resolve_attacks_for_investigator_fires_engaged_ready_enemy_and_exhausts() {
    let enemy_id = EnemyId(1);
    let session = attacked_on_end_turn(test_support::seat(1), vec![attacker(1, "Test Enemy", 1)])
        .session()
        .take(&TurnAction::EndTurn);
    let events = session.events();

    // Damage placed.
    assert_event!(events, Event::DamageTaken { investigator, amount: 1 } if *investigator == INV);

    // Exhaust asserted on the event, not on post-drive state: the cascade
    // past the attack step runs on into Upkeep, which readies every
    // exhausted enemy again. The event is the record of the moment.
    assert_event!(events, Event::EnemyExhausted { enemy } if *enemy == enemy_id);

    // Ordering: DamageTaken precedes EnemyExhausted (post-attack exhaust).
    assert_event_sequence!(
        events,
        Event::DamageTaken { .. },
        Event::EnemyExhausted { .. },
    );
}
#[test]
fn resolve_attacks_for_investigator_excludes_exhausted_and_unengaged_enemies() {
    // Engaged but exhausted — must NOT attack.
    let mut e1 = attacker(1, "Exhausted Engaged", 5);
    e1.exhausted = true;

    // Ready but unengaged (and off the board) — must NOT attack.
    let e2 = attacker(2, "Ready Unengaged", 5);

    // Ready engaged — the only one that attacks.
    let e3 = attacker(3, "Ready Engaged", 1);

    let session = attacked_on_end_turn(test_support::seat(1), vec![e1, e3])
        .with_enemy(e2)
        .session()
        .take(&TurnAction::EndTurn);
    let events = session.events();

    // Exactly one DamageTaken (from e3, amount 1).
    assert_event_count!(events, 1, Event::DamageTaken { .. });
    assert_event!(events, Event::DamageTaken { amount: 1, .. });

    // Only e3 exhausted (e1 was already, and does not re-emit; e2 never
    // attacked). Asserted on events rather than post-drive state — the
    // cascade past the attack step reaches Upkeep, which readies enemies.
    assert_event_count!(events, 1, Event::EnemyExhausted { .. });
    assert_event!(events, Event::EnemyExhausted { enemy: EnemyId(3) });
}
#[test]
fn resolve_attacks_for_investigator_pick_overrides_enemy_id_order() {
    // Inserted in non-id order: the attackers are still snapshotted 2 then 10.
    let session = attacked_on_end_turn(
        test_support::seat(1), // TEST_INV: 8 health; 1+2=3 total damage < 8
        vec![
            attacker(10, "Higher id", 2), // EnemyId(10), dmg 2
            attacker(2, "Lower id", 1),   // EnemyId(2), dmg 1
        ],
    )
    .session()
    // 2 ready engaged enemies → suspend on the order pick (#143), not EnemyId order.
    .take(&TurnAction::EndTurn);

    // Options are the snapshotted attackers in EnemyId order.
    let offered: Vec<_> = session
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect();
    assert_eq!(
        offered,
        vec![
            Some(OptionTarget::Enemy(EnemyId(2))),
            Some(OptionTarget::Enemy(EnemyId(10))),
        ],
        "the attackers are offered in EnemyId order"
    );

    // Pick the higher-id enemy (dmg 2) to strike FIRST, proving the player's
    // pick overrides the deterministic order. The cascade then runs on past
    // the attack step; what this test pins is the order the two attacks
    // landed in.
    let session = session.pick(OptionTarget::Enemy(EnemyId(10)));
    // Both attacks resolved; the chosen (EnemyId 10, dmg 2) struck first.
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
        vec![2, 1],
        "chosen EnemyId(10) (dmg 2) attacked before EnemyId(2) (dmg 1)"
    );
    let exhausted: Vec<EnemyId> = session
        .events()
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
    // EnemyId(1) deals the killing blow on its attack; EnemyId(2) must NOT
    // attack (active check fails at loop top). 7 damage pre-loaded: remaining
    // health = 1 (max_health()=8 from TEST_INV; 7+1=8=defeated).
    let session = attacked_on_end_turn(
        test_support::seat(1).harm(7, 0),
        vec![attacker(1, "Killer", 1), attacker(2, "Bystander", 5)],
    )
    .session()
    // 2 engaged → order pick first (#143).
    .take(&TurnAction::EndTurn);

    // Pick EnemyId(1) (the killer) to strike first; after it defeats the
    // investigator, the active check at the loop top early-breaks before any
    // re-prompt, so EnemyId(2) never attacks.
    let result = session.pick(OptionTarget::Enemy(EnemyId(1))).finish();
    let (state, events) = (&result.state, &result.events);

    // e1's attack killed the sole investigator, so the scenario's resolution
    // latched and the parked attack loop is one of the frames that cancels
    // (ADR 0004 — `cancelled_by_scenario_end`). Nothing further in the round
    // happens, including e1's own exhaust, which since #704 follows its
    // attack rather than being part of it.
    assert_eq!(result.outcome, EngineOutcome::Done, "the game is over");
    assert!(
        state.ending.is_some(),
        "the sole investigator's defeat ended the scenario"
    );
    assert!(
        !state.enemies[&EnemyId(1)].exhausted && !state.enemies[&EnemyId(2)].exhausted,
        "the ended scenario cancels the rest of the loop"
    );

    assert_event_count!(events, 1, Event::DamageTaken { .. });

    // No exhaust at all: e1's attack ended the scenario before its own
    // post-attack exhaust step ran (see the assertions above), and e2 never
    // attacked.
    assert_no_event!(events, Event::EnemyExhausted { .. });

    // Investigator was defeated.
    assert_eq!(state.investigators[&INV].status, Status::Defeated);
}
#[test]
fn enemy_phase_emits_phase_started_and_cascades_to_mythos_in_no_eligibility_case() {
    // 1 Active investigator, no engaged enemies. Auto-skip
    // cascades through both windows + enemy_phase_end +
    // Upkeep → Mythos.
    let inv_id = InvestigatorId(1);
    let mut state = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
        .with_turn_order([inv_id])
        .with_phase(Phase::Investigation)
        .build();
    let mut events = Vec::new();

    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Investigation → Enemy

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
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_enemy(e1)
        .with_enemy(e2)
        .with_turn_order([id1, id2])
        .with_phase(Phase::Investigation)
        .build();
    let mut events = Vec::new();

    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Investigation → Enemy

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
        .with_seat(test_support::seat(1))
        .with_seat(test_support::seat(2))
        .with_seat(test_support::seat(3))
        .with_enemy(e1)
        .with_enemy(e2)
        .with_enemy(e3)
        .with_turn_order([id1, id2, id3])
        .with_phase(Phase::Investigation)
        .build();
    state.investigators.get_mut(&id2).unwrap().status = Status::Defeated;
    let mut events = Vec::new();

    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Investigation → Enemy

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
        .with_seat(test_support::seat(1))
        .with_turn_order([id1])
        .with_phase(Phase::Investigation)
        .build();
    state.investigators.get_mut(&id1).unwrap().status = Status::Defeated;
    let mut events = Vec::new();

    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Investigation → Enemy

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
        .with_seat(test_support::seat(1))
        .with_enemy(enemy)
        .with_turn_order([inv_id])
        .with_phase(Phase::Investigation)
        .build();
    let mut events = Vec::new();

    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Investigation → Enemy

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
        .with_seat(test_support::seat(1))
        .with_turn_order([InvestigatorId(1)])
        .with_phase(Phase::Enemy)
        .build();
    // Use a state where Upkeep's cascade can complete (Active investigator exists).
    let mut events = Vec::new();

    step_phase(&mut Cx::for_test(&mut state, &mut events)); // Enemy → Upkeep

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
    // Skipping routes through resolve_input's "open_windows non-empty +
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
    let session = GameStateBuilder::default()
        .with_seat(test_support::seat(1))
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
        .session()
        .skip();

    // The Skip resumes the continuation; the cascade runs into Mythos and
    // pauses at the step-1.4 encounter-draw prompt.
    session.assert_at_encounter_draw();
    let state = session.state();
    assert_eq!(
        state.phase,
        Phase::Mythos,
        "cascade lands in Mythos after Skip resumed the continuation"
    );
    assert_event!(
        session.events(),
        Event::DamageTaken { investigator, amount: 1 } if *investigator == inv_id
    );
    assert_event!(session.events(), Event::EnemyExhausted { enemy } if *enemy == enemy_id);
    // No stranded frame: a stack invariant, so read the stack.
    assert!(
        !state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EnemyPhase(_))),
        "the EnemyPhase anchor (with its `attacking` cursor) is gone after the \
         continuation advances past the last Active investigator and the AfterAll \
         window auto-skips"
    );
}
