//! A hunter's move is resolved by the player when the shortest path ties, and
//! that choice replays identically.
//!
//! The substrate is map **topology**, not a card: a symmetric diamond producing
//! a genuine two-way tie in the hunter's first step. A hand-built fixture is the
//! right one here — ADR 0016 permits one that models an engine primitive, which
//! a bare connection graph is. The Gathering's hub-and-spoke layout has no
//! analogue, which is why the map stays hand-built rather than flipping to real
//! scenario content.
//!
//! Renamed on the way down from `crates/scenarios/tests/hunter_movement.rs`
//! (#873): the surviving test is about the tie and the `PickSingle` that settles
//! it, not about hunter movement at large. The spawn-engagement tie that also
//! lived in that file went to real cards under #877.

use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{self, OptionId};
use game_core::state::{
    Continuation, EnemyId, GameState, InvestigationResume, InvestigatorId, LocationId, Phase,
};
use game_core::test_support::{self, GameStateBuilder, MockRegistry};

#[ctor::ctor(unsafe)]
fn install() {
    // No probe cards: the diamond is pure topology. `install`'s composed
    // `metadata_for_test_inv` is what `test_investigator`'s capacity reads want.
    MockRegistry::new().install();
}

#[test]
fn hunter_move_tie_break_replays_identically() {
    fn diamond_state() -> GameState {
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
        GameStateBuilder::new()
            .with_phase(Phase::Investigation)
            .with_location(loc_a)
            .with_location(loc_b)
            .with_location(loc_c)
            .with_location(loc_d)
            .with_investigator(inv)
            .with_active_investigator(InvestigatorId(1))
            .with_turn_order([InvestigatorId(1)])
            .with_enemy(hunter)
            // Mid-Investigation invariant (slice 1a): the end_turn cascade pops
            // the InvestigationPhase anchor at investigation_phase_end.
            .with_phase_anchor(Continuation::InvestigationPhase {
                resume: InvestigationResume::TurnBegins,
            })
            // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame
            // the end_turn cascade pops before advancing into the Enemy phase.
            .with_investigator_turn(InvestigatorId(1))
            .build()
    }

    // Candidates are the sorted first-steps toward D: [LocationId(2), LocationId(3)],
    // so LocationId(3) is offered option id 1.

    let mut s1 = diamond_state();
    s1 = test_support::take_turn_action(s1, &TurnAction::EndTurn).state;
    s1 = engine::apply(
        s1,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }),
    )
    .state;
    let mut s2 = diamond_state();
    s2 = test_support::take_turn_action(s2, &TurnAction::EndTurn).state;
    s2 = engine::apply(
        s2,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }),
    )
    .state;
    // Replay determinism is a whole-state property: replaying an
    // identical action log reproduces state bit-for-bit.
    assert_eq!(
        s1, s2,
        "replaying the same action log must reproduce identical state",
    );
    assert_eq!(
        s1.enemies[&EnemyId(1)].current_location,
        Some(LocationId(3))
    );
}
