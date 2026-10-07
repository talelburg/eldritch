use super::*;
use crate::state::GameStateBuilder;

#[test]
fn enemy_phase_anchor_attacking_round_trips_through_serde() {
    let mut state = GameStateBuilder::new().build();
    state.continuations.push(Continuation::EnemyPhase {
        resume: EnemyResume::BeforeInvestigatorAttacked,
        attacking: Some(InvestigatorId(7)),
    });
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.continuations, state.continuations);
}

#[test]
fn deal_damage_frame_round_trips_through_serde() {
    let mut state = GameStateBuilder::new().build();
    state.continuations.push(DealDamageFrame {
        investigator: InvestigatorId(1),
        source: DamageSource::EnemyAttack { enemy: EnemyId(5) },
        assignment: Assignment::default(),
        step: DealDamageStep::Distribute {
            remaining_damage: 2,
            remaining_horror: 0,
        },
    });
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.continuations, state.continuations);
}

#[test]
fn attack_loop_frame_round_trips_through_serde() {
    let mut state = GameStateBuilder::new().build();
    state.continuations.push(AttackLoopFrame {
        investigator: InvestigatorId(7),
        remaining_attackers: vec![EnemyId(2), EnemyId(3)],
        source: EnemyAttackSource::EnemyPhase,
        stage: AttackLoopStage::Attacking,
    });
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.continuations, state.continuations);
}

#[test]
fn attack_loop_pick_order_stage_round_trips_through_serde() {
    let mut state = GameStateBuilder::new().build();
    state.continuations.push(AttackLoopFrame {
        investigator: InvestigatorId(1),
        remaining_attackers: vec![EnemyId(2), EnemyId(3)],
        source: EnemyAttackSource::EnemyPhase,
        stage: AttackLoopStage::PickOrder,
    });
    let json = serde_json::to_string(&state).expect("serialize");
    let back: GameState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.continuations, state.continuations);
}
