use super::*;
use crate::engine::{dispatch, enumerate::TurnAction, outcome::OptionTarget, Cx};
use crate::state::{EnemyId, EnemyPhaseFrame, EnemyResume, InvestigatorId, LocationId, Phase};
use crate::{assert_event, test_support};

use card_dsl::card_data::SkillKind;

use crate::state::GameStateBuilder;

#[test]
fn hunter_move_tie_suspends_then_resumes_on_pick_location() {
    // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
    // default prey. Ending the turn runs the Enemy phase, where two equal
    // first-steps (B, C) suspend for the lead investigator's pick.
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
    let session = GameStateBuilder::new()
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_investigator(inv)
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn);
    assert_eq!(session.state().phase, Phase::Enemy);

    // Pick C. Resolving the tie continues the Enemy phase; with no registry
    // the attack windows auto-skip and the cascade runs to Mythos, pausing at
    // the step-1.4 encounter-draw prompt.
    let session = session.pick(OptionTarget::Location(LocationId(3)));
    assert_eq!(session.prompt().target, Some(OptionTarget::EncounterDeck));
    assert_eq!(
        session.state().enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
    assert_event!(session.events(), Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == LocationId(3));
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
        matches!(state.continuations.top(), Some(Continuation::HunterMove(_))),
        "pending stays open on invalid pick"
    );
}

#[test]
fn hunter_engage_tie_suspends_then_resumes_on_pick_investigator() {
    // Two investigators at B; ending both turns runs the Enemy phase, where
    // the hunter moves A->B and default prey ties -> PickSingle.
    // Default prey reads remaining health, which comes from the registry.
    test_support::install_test_registry();
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
    let session = GameStateBuilder::new()
        .with_location(a)
        .with_location(b)
        .with_investigator(i1)
        .with_investigator(i2)
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(h)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn)
        .take(&TurnAction::EndTurn);
    // Moved to B already, suspended on the engagement tie.
    assert_eq!(
        session.state().enemies[&EnemyId(1)].current_location,
        Some(LocationId(2))
    );

    // The tied investigators are offered on their own investigator cards.
    let second = session.state().investigators[&InvestigatorId(2)].card_anchor();
    let session = session.pick(second);
    // Resolving the tie continues the Enemy phase through to Mythos's
    // encounter-draw prompt.
    assert_eq!(session.prompt().target, Some(OptionTarget::EncounterDeck));
    assert_eq!(
        session.state().enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(2))
    );
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
    // The Enemy phase suspends on hunter1; the pick takes B; then hunter2
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
    let session = GameStateBuilder::new()
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_investigator(inv)
        .with_enemy(tie_hunter)
        .with_enemy(clean_hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn)
        // Resolve hunter 1's tie -> hunter 2 then moves B->D and engages.
        .pick(OptionTarget::Location(LocationId(2)));
    // The cascade runs on to Mythos's encounter-draw prompt.
    assert_eq!(session.prompt().target, Some(OptionTarget::EncounterDeck));
    assert_eq!(
        session.state().enemies[&EnemyId(2)].current_location,
        Some(LocationId(4))
    );
    assert_eq!(
        session.state().enemies[&EnemyId(2)].engaged_with,
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
        .with_phase_anchor(EnemyPhaseFrame {
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
        state.continuations.top(),
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
        matches!(state.continuations.top(), Some(Continuation::HunterMove(_))),
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
        state.continuations.top(),
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
        matches!(state.continuations.top(), Some(Continuation::HunterMove(_))),
        "pending preserved so client can retry with PickSingle"
    );
}
