use super::*;
use crate::engine::{dispatch, Cx};
use crate::state::{EnemyId, EnemyResume, InvestigatorId, LocationId, Phase};
use crate::{assert_event, test_support};

/// Build the `PickSingle` response selecting the offered option whose label
/// is `format!("{target:?}")`, from a suspended `AwaitingInput`'s options.
/// Panics if no option matches (a test-setup error).
fn pick(outcome: &EngineOutcome, target: impl Debug) -> InputResponse {
    let EngineOutcome::AwaitingInput { request, .. } = outcome else {
        panic!("expected AwaitingInput, got {outcome:?}");
    };
    let label = format!("{target:?}");
    let opt = request
        .options
        .iter()
        .find(|o| o.label == label)
        .unwrap_or_else(|| {
            panic!(
                "no offered option labeled {label:?} in {:?}",
                request.options
            )
        });
    InputResponse::PickSingle(opt.id)
}
use card_dsl::card_data::SkillKind;

use crate::state::GameStateBuilder;

#[test]
fn hunter_move_tie_suspends_then_resumes_on_pick_location() {
    // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
    // default prey. Two equal first-steps (B, C) -> AwaitingInput.
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
        .with_phase(Phase::Enemy)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(hunter)
        // EnemyPhase anchor (slice 1a): the resume cascade reaches the
        // attack kickoff / enemy_phase_end, which require it.
        .with_phase_anchor(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: None,
        })
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(matches!(
        state.continuations.last(),
        Some(Continuation::HunterMove(_))
    ));
    // Resume by picking C.
    let mut ev2 = Vec::new();
    let resumed = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut ev2,
        };
        // resolve_input resumes the tie; drive then carries the Enemy→Upkeep
        // →Mythos cascade forward (slice 1b), as the apply boundary does.
        let o = dispatch::resolve_input(&mut cx, &pick(&outcome, LocationId(3)));
        dispatch::drive(&mut cx, o)
    };
    // Resolving the tie continues the Enemy phase; with no registry the
    // attack windows auto-skip and the cascade runs to Mythos, pausing at
    // the step-1.4 encounter-draw prompt (AwaitingInput).
    assert!(matches!(resumed, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
    assert!(!matches!(
        state.continuations.last(),
        Some(Continuation::HunterMove(_))
    ));
    assert_event!(ev2, Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(3));
}

#[test]
fn hunter_move_tie_rejects_invalid_pick() {
    // Same diamond setup; resume with a location not in candidates.
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
        .with_phase(Phase::Enemy)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(hunter)
        .build();
    let mut events = Vec::new();
    drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    let mut ev2 = Vec::new();
    // Option id 99 is out of the candidate range -> rejected.
    let result = dispatch::resolve_input(
        &mut Cx {
            state: &mut state,
            events: &mut ev2,
        },
        &InputResponse::PickSingle(OptionId(99)),
    );
    assert!(matches!(result, EngineOutcome::Rejected { .. }));
    assert!(
        matches!(
            state.continuations.last(),
            Some(Continuation::HunterMove(_))
        ),
        "pending stays open on invalid pick"
    );
}

#[test]
fn hunter_engage_tie_suspends_then_resumes_on_pick_investigator() {
    // Two investigators at B; hunter moves A->B; default prey -> tie ->
    // PickSingle.
    let mut a = test_support::test_location(1, "A");
    let mut b = test_support::test_location(2, "B");
    a.connections = vec![LocationId(2)];
    b.connections = vec![LocationId(1)];
    let mut i1 = test_support::test_investigator(1);
    i1.current_location = Some(LocationId(2));
    let mut i2 = test_support::test_investigator(2);
    i2.current_location = Some(LocationId(2));
    let mut h = test_support::test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(a)
        .with_location(b)
        .with_investigator(i1)
        .with_investigator(i2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(h)
        // EnemyPhase anchor (slice 1a): resume cascades into the attack
        // kickoff / enemy_phase_end.
        .with_phase_anchor(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: None,
        })
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    // Moved to B already, suspended on engagement tie.
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    let mut ev2 = Vec::new();
    let resumed = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut ev2,
        };
        let o = dispatch::resolve_input(&mut cx, &pick(&outcome, InvestigatorId(2)));
        dispatch::drive(&mut cx, o) // slice 1b: complete the Enemy→… cascade
    };
    // Resolving the tie continues the Enemy phase; with no registry the
    // attack windows auto-skip and the cascade runs to Mythos, pausing at
    // the step-1.4 encounter-draw prompt (AwaitingInput).
    assert!(matches!(resumed, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(2))
    );
    assert!(!matches!(
        state.continuations.last(),
        Some(Continuation::HunterMove(_))
    ));
}

#[test]
fn highest_combat_prey_breaks_move_tie_without_prompt() {
    // Fan A(1)-{B(2),C(3)}. inv1 at B combat 5; inv2 at C combat 2.
    // hunter at A with Ranked Highest-combat prey. resolve_prey picks
    // inv1 unambiguously -> moves A->B, engages, no prompt.
    let mut loc_a = test_support::test_location(1, "A");
    let mut loc_b = test_support::test_location(2, "B");
    let mut loc_c = test_support::test_location(3, "C");
    loc_a.connections = vec![LocationId(2), LocationId(3)];
    loc_b.connections = vec![LocationId(1)];
    loc_c.connections = vec![LocationId(1)];
    let mut inv1 = test_support::test_investigator(1);
    inv1.current_location = Some(LocationId(2));
    inv1.skills.combat = 5;
    let mut inv2 = test_support::test_investigator(2);
    inv2.current_location = Some(LocationId(3));
    inv2.skills.combat = 2;
    let mut hunter = test_support::test_enemy(1, "Ghoul Priest");
    hunter.hunter = true;
    hunter.prey = Prey::Ranked {
        direction: PreyDirection::Highest,
        measure: PreyMeasure::Skill(SkillKind::Combat),
    };
    hunter.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_investigator(inv1)
        .with_investigator(inv2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(hunter)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert_eq!(outcome, EngineOutcome::Done);
    // Moves toward inv1 (B) and engages immediately (arrives at B).
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1))
    );
}

#[test]
fn multi_hunter_one_suspends_then_next_processed_on_resume() {
    // Diamond A(1)-{B(2),C(3)}-D(4). inv at D(4).
    // Hunter1 at A(1) ties B/C; hunter2 at B(2) has clean B->D step.
    // drive suspends on hunter1; resume picks B; then hunter2
    // processes automatically: moves B->D and engages.
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
    let mut tie_hunter = test_support::test_enemy(1, "Tie Hunter");
    tie_hunter.hunter = true;
    tie_hunter.current_location = Some(LocationId(1)); // ties B/C toward D
    let mut clean_hunter = test_support::test_enemy(2, "Clean Hunter");
    clean_hunter.hunter = true;
    clean_hunter.current_location = Some(LocationId(2)); // single step B->D
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(tie_hunter)
        .with_enemy(clean_hunter)
        // EnemyPhase anchor (slice 1a): resume cascades into the attack
        // kickoff / enemy_phase_end.
        .with_phase_anchor(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: None,
        })
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    // Resolve hunter 1's tie -> hunter 2 then moves B->D and engages.
    let mut ev2 = Vec::new();
    let resumed = {
        let mut cx = Cx {
            state: &mut state,
            events: &mut ev2,
        };
        let o = dispatch::resolve_input(&mut cx, &pick(&outcome, LocationId(2)));
        dispatch::drive(&mut cx, o) // slice 1b: complete the Enemy→… cascade
    };
    // Resolving the tie continues the Enemy phase; with no registry the
    // attack windows auto-skip and the cascade runs to Mythos, pausing at
    // the step-1.4 encounter-draw prompt (AwaitingInput).
    assert!(matches!(resumed, EngineOutcome::AwaitingInput { .. }));
    assert_eq!(
        state.enemies[&EnemyId(2)].current_location,
        Some(LocationId(4))
    );
    assert_eq!(
        state.enemies[&EnemyId(2)].engaged_with,
        Some(InvestigatorId(1))
    );
}

#[test]
fn hunter_move_tie_rejects_wrong_response_kind() {
    // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
    // default prey. Two equal first-steps (B, C) -> AwaitingInput on Move.
    // Client submits a non-PickSingle response (Skip) -> Rejected,
    // pending preserved for retry.
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
        .with_phase(Phase::Enemy)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_investigator(inv)
        .with_turn_order([InvestigatorId(1)])
        .with_enemy(hunter)
        // EnemyPhase anchor (slice 1a): the resume cascade reaches the
        // attack kickoff / enemy_phase_end, which require it.
        .with_phase_anchor(Continuation::EnemyPhase {
            resume: EnemyResume::BeforeInvestigatorAttacked,
            attacking: None,
        })
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(matches!(
        state.continuations.last(),
        Some(Continuation::HunterMove(_))
    ));
    // Submit a non-PickSingle response (Skip).
    let mut ev2 = Vec::new();
    let result = dispatch::resolve_input(
        &mut Cx {
            state: &mut state,
            events: &mut ev2,
        },
        &InputResponse::Skip,
    );
    assert!(
        matches!(result, EngineOutcome::Rejected { .. }),
        "wrong response kind should be rejected"
    );
    assert!(
        matches!(
            state.continuations.last(),
            Some(Continuation::HunterMove(_))
        ),
        "pending preserved so client can retry with PickSingle"
    );
}

#[test]
fn hunter_engage_tie_rejects_wrong_response_kind() {
    // Two investigators at B(2); hunter moves A(1)->B(2); default prey
    // -> engage tie -> AwaitingInput on Engage.
    // Client submits a non-PickSingle response (Skip) -> Rejected,
    // pending preserved for retry.
    let mut loc_a = test_support::test_location(1, "A");
    let mut loc_b = test_support::test_location(2, "B");
    loc_a.connections = vec![LocationId(2)];
    loc_b.connections = vec![LocationId(1)];
    let mut inv1 = test_support::test_investigator(1);
    inv1.current_location = Some(LocationId(2));
    let mut inv2 = test_support::test_investigator(2);
    inv2.current_location = Some(LocationId(2));
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(LocationId(1));
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Enemy)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_investigator(inv1)
        .with_investigator(inv2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(hunter)
        .build();
    let mut events = Vec::new();
    let outcome = drive_hunter_moves(&mut Cx {
        state: &mut state,
        events: &mut events,
    });
    // Moved to B already, suspended on engagement tie.
    assert_eq!(
        state.enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );
    assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
    assert!(matches!(
        state.continuations.last(),
        Some(Continuation::HunterMove(_))
    ));
    // Submit a non-PickSingle response (Skip).
    let mut ev2 = Vec::new();
    let result = dispatch::resolve_input(
        &mut Cx {
            state: &mut state,
            events: &mut ev2,
        },
        &InputResponse::Skip,
    );
    assert!(
        matches!(result, EngineOutcome::Rejected { .. }),
        "wrong response kind should be rejected"
    );
    assert!(
        matches!(
            state.continuations.last(),
            Some(Continuation::HunterMove(_))
        ),
        "pending preserved so client can retry with PickSingle"
    );
}
