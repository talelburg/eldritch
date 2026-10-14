use super::*;
use crate::action::{Action, PlayerAction};
use crate::engine::{enumerate::TurnAction, outcome::OptionTarget};
use crate::state::{EnemyId, InvestigatorId, LocationId, Phase};
use crate::{assert_event, test_support};

use card_dsl::card_data::SkillKind;

use crate::state::GameStateBuilder;
use crate::test_support::TEST_REGISTRIES;

/// Location "A".
const A: LocationId = LocationId(1);
/// Location "B".
const B: LocationId = LocationId(2);
/// Location "C".
const C: LocationId = LocationId(3);
/// Location "D".
const D: LocationId = LocationId(4);
#[test]
fn hunter_move_tie_suspends_then_resumes_on_pick_location() {
    // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
    // default prey. Ending the turn runs the Enemy phase, where two equal
    // first-steps (B, C) suspend for the lead investigator's pick.
    let mut loc_a = test_support::test_location(A.0, "A");
    let mut loc_b = test_support::test_location(B.0, "B");
    let mut loc_c = test_support::test_location(C.0, "C");
    let mut loc_d = test_support::test_location(D.0, "D");
    loc_a.connections = vec![B, C];
    loc_b.connections = vec![A, D];
    loc_c.connections = vec![A, D];
    loc_d.connections = vec![B, C];
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(A);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_seat(test_support::seat(1).at(D))
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn);
    assert_eq!(session.state().phase, Phase::Enemy);

    // Pick C. Resolving the tie continues the Enemy phase; with no ability
    // the attack windows auto-skip and the cascade runs to Mythos, pausing at
    // the step-1.4 encounter-draw prompt.
    let session = session.pick(OptionTarget::Location(C));
    session.assert_at_encounter_draw();
    assert_eq!(
        session.state().enemies[&EnemyId(1)].current_location,
        Some(C)
    );
    assert_event!(session.events(), Event::EnemyMoved { enemy, to } if *enemy == EnemyId(1) && *to == C);
}

#[test]
fn hunter_move_tie_rejects_invalid_pick() {
    // Same diamond setup; resume with a location not in candidates.
    let mut loc_a = test_support::test_location(A.0, "A");
    let mut loc_b = test_support::test_location(B.0, "B");
    let mut loc_c = test_support::test_location(C.0, "C");
    let mut loc_d = test_support::test_location(D.0, "D");
    loc_a.connections = vec![B, C];
    loc_b.connections = vec![A, D];
    loc_c.connections = vec![A, D];
    loc_d.connections = vec![B, C];
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(A);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_seat(test_support::seat(1).at(D))
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn)
        // Option id 99 is out of the candidate range -> rejected. A malformed
        // wire response no step can name, so the raw `ResolveInput` is sent.
        .apply(Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(99)),
        }));
    session.expect_rejected();
    // Pending stays open on invalid pick: both routes are still offered.
    assert_eq!(session.offered_at(&OptionTarget::Location(B)), 1);
    assert_eq!(session.offered_at(&OptionTarget::Location(C)), 1);
}
#[test]
fn hunter_engage_tie_suspends_then_resumes_on_pick_investigator() {
    // Two investigators at B; ending both turns runs the Enemy phase, where
    // the hunter moves A->B and default prey ties -> PickSingle.
    let mut a = test_support::test_location(A.0, "A");
    let mut b = test_support::test_location(B.0, "B");
    a.connections = vec![B];
    b.connections = vec![A];
    let mut h = test_support::test_enemy(1, "Hunter");
    h.hunter = true;
    h.current_location = Some(A);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(a)
        .with_location(b)
        .with_seat(test_support::seat(1).at(B))
        .with_seat(test_support::seat(2).at(B))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(h)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn)
        .take(&TurnAction::EndTurn);
    // Moved to B already, suspended on the engagement tie.
    assert_eq!(
        session.state().enemies[&EnemyId(1)].current_location,
        Some(B)
    );

    // The tied investigators are offered on their own investigator cards.
    let second = session.state().investigators[&InvestigatorId(2)].card_anchor();
    let session = session.pick(second);
    // Resolving the tie continues the Enemy phase through to Mythos's
    // encounter-draw prompt.
    session.assert_at_encounter_draw();
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
    let mut loc_a = test_support::test_location(A.0, "A");
    let mut loc_b = test_support::test_location(B.0, "B");
    let mut loc_c = test_support::test_location(C.0, "C");
    loc_a.connections = vec![B, C];
    loc_b.connections = vec![A];
    loc_c.connections = vec![A];
    let mut hunter = test_support::test_enemy(1, "Ghoul Priest");
    hunter.hunter = true;
    hunter.prey = Prey::Ranked {
        direction: PreyDirection::Highest,
        measure: PreyMeasure::Skill(SkillKind::Combat),
    };
    hunter.current_location = Some(A);
    // Ending both turns runs the Enemy phase's hunter step.
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_seat(test_support::seat(1).at(B).skill(SkillKind::Combat, 5))
        .with_seat(test_support::seat(2).at(C).skill(SkillKind::Combat, 2))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn)
        .take(&TurnAction::EndTurn);
    // No prompt: the round runs on to Mythos's encounter-draw prompt.
    session.assert_at_encounter_draw();
    // Moves toward inv1 (B) and engages immediately (arrives at B).
    assert_eq!(
        session.state().enemies[&EnemyId(1)].current_location,
        Some(B)
    );
    assert_eq!(
        session.state().enemies[&EnemyId(1)].engaged_with,
        Some(InvestigatorId(1))
    );
}
#[test]
fn multi_hunter_one_suspends_then_next_processed_on_resume() {
    // Diamond A(1)-{B(2),C(3)}-D(4). inv at D(4).
    // Hunter1 at A(1) ties B/C; hunter2 at B(2) has clean B->D step.
    // The Enemy phase suspends on hunter1; the pick takes B; then hunter2
    // processes automatically: moves B->D and engages.
    let mut loc_a = test_support::test_location(A.0, "A");
    let mut loc_b = test_support::test_location(B.0, "B");
    let mut loc_c = test_support::test_location(C.0, "C");
    let mut loc_d = test_support::test_location(D.0, "D");
    loc_a.connections = vec![B, C];
    loc_b.connections = vec![A, D];
    loc_c.connections = vec![A, D];
    loc_d.connections = vec![B, C];
    let mut tie_hunter = test_support::test_enemy(1, "Tie Hunter");
    tie_hunter.hunter = true;
    tie_hunter.current_location = Some(A); // ties B/C toward D
    let mut clean_hunter = test_support::test_enemy(2, "Clean Hunter");
    clean_hunter.hunter = true;
    clean_hunter.current_location = Some(B); // single step B->D
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_seat(test_support::seat(1).at(D))
        .with_enemy(tie_hunter)
        .with_enemy(clean_hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn)
        // Resolve hunter 1's tie -> hunter 2 then moves B->D and engages.
        .pick(OptionTarget::Location(B));
    // The cascade runs on to Mythos's encounter-draw prompt.
    session.assert_at_encounter_draw();
    assert_eq!(
        session.state().enemies[&EnemyId(2)].current_location,
        Some(D)
    );
    assert_eq!(
        session.state().enemies[&EnemyId(2)].engaged_with,
        Some(InvestigatorId(1))
    );
}

#[test]
fn hunter_move_tie_rejects_wrong_response_kind() {
    // Diamond A(1)-{B(2),C(3)}-D(4). Investigator at D; hunter at A,
    // default prey. Two equal first-steps (B, C) -> a pick on the move.
    // Client submits a non-PickSingle response (Skip) -> Rejected,
    // pending preserved for retry.
    let mut loc_a = test_support::test_location(A.0, "A");
    let mut loc_b = test_support::test_location(B.0, "B");
    let mut loc_c = test_support::test_location(C.0, "C");
    let mut loc_d = test_support::test_location(D.0, "D");
    loc_a.connections = vec![B, C];
    loc_b.connections = vec![A, D];
    loc_c.connections = vec![A, D];
    loc_d.connections = vec![B, C];
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(A);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_location(loc_c)
        .with_location(loc_d)
        .with_seat(test_support::seat(1).at(D))
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn);
    assert_eq!(session.offered_at(&OptionTarget::Location(B)), 1);
    // Submit a non-PickSingle response (Skip).
    let session = session.skip();
    session.expect_rejected();
    // Pending preserved so client can retry with a pick.
    assert_eq!(session.offered_at(&OptionTarget::Location(B)), 1);
    assert_eq!(session.offered_at(&OptionTarget::Location(C)), 1);
}
#[test]
fn hunter_engage_tie_rejects_wrong_response_kind() {
    // Two investigators at B(2); ending both turns runs the Enemy phase,
    // where the hunter moves A(1)->B(2); default prey -> engage tie -> a
    // pick on the engagement.
    // Client submits a non-PickSingle response (Skip) -> Rejected,
    // pending preserved for retry.
    let mut loc_a = test_support::test_location(A.0, "A");
    let mut loc_b = test_support::test_location(B.0, "B");
    loc_a.connections = vec![B];
    loc_b.connections = vec![A];
    let mut hunter = test_support::test_enemy(1, "Hunter");
    hunter.hunter = true;
    hunter.current_location = Some(A);
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(loc_a)
        .with_location(loc_b)
        .with_seat(test_support::seat(1).at(B))
        .with_seat(test_support::seat(2).at(B))
        .with_turn_order([InvestigatorId(1), InvestigatorId(2)])
        .with_enemy(hunter)
        .open_turn(InvestigatorId(1))
        .session()
        .take(&TurnAction::EndTurn)
        .take(&TurnAction::EndTurn);
    // Moved to B already, suspended on engagement tie.
    assert_eq!(
        session.state().enemies[&EnemyId(1)].current_location,
        Some(B)
    );
    let tied: Vec<_> = [InvestigatorId(1), InvestigatorId(2)]
        .map(|id| session.state().investigators[&id].card_anchor())
        .into();
    for anchor in &tied {
        assert_eq!(session.offered_at(anchor), 1);
    }
    // Submit a non-PickSingle response (Skip).
    let session = session.skip();
    session.expect_rejected();
    // Pending preserved so client can retry with a pick.
    for anchor in &tied {
        assert_eq!(session.offered_at(anchor), 1);
    }
    assert_eq!(session.state().enemies[&EnemyId(1)].engaged_with, None);
}
