//! Hunter-movement replay equality across a `PickSingle` round-trip.
//!
//! The substrate is map **topology**, not a card: a symmetric diamond producing
//! a genuine two-way tie in the hunter's first step. ADR 0016 permits a
//! hand-built fixture that models an engine primitive, which a bare connection
//! graph is.
//!
//! The spawn-engagement tie that also lived here moved to
//! `crates/cards/tests/spawn_engagement_tie.rs` (#877), where it runs against
//! Flesh-Eater 01118 and the real registry.

use game_core::action::{InputResponse, PlayerAction};
use game_core::engine::{apply, OptionId};
use game_core::state::{EnemyId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{Action, TurnAction};
use scenarios::test_fixtures::synth_cards::TEST_REGISTRY;

#[ctor::ctor(unsafe)]
fn install_test_registry() {
    let _ = game_core::card_registry::install(TEST_REGISTRY);
}

#[test]
fn hunter_movement_pick_location_replays_identically() {
    fn diamond_state() -> game_core::state::GameState {
        let mut loc_a = test_location(1, "A");
        let mut loc_b = test_location(2, "B");
        let mut loc_c = test_location(3, "C");
        let mut loc_d = test_location(4, "D");
        loc_a.connections = vec![LocationId(2), LocationId(3)];
        loc_b.connections = vec![LocationId(1), LocationId(4)];
        loc_c.connections = vec![LocationId(1), LocationId(4)];
        loc_d.connections = vec![LocationId(2), LocationId(3)];
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(4));
        let mut hunter = test_enemy(1, "Hunter");
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
            .with_phase_anchor(game_core::state::Continuation::InvestigationPhase {
                resume: game_core::state::InvestigationResume::TurnBegins,
            })
            // Open-turn invariant (slice 2a-i, #393): the InvestigatorTurn frame
            // the end_turn cascade pops before advancing into the Enemy phase.
            .with_investigator_turn(InvestigatorId(1))
            .build()
    }

    // Candidates are the sorted first-steps toward D: [LocationId(2), LocationId(3)],
    // so LocationId(3) is offered option id 1.

    let mut s1 = diamond_state();
    s1 = take_turn_action(s1, &TurnAction::EndTurn).state;
    s1 = apply(
        s1,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(1)),
        }),
    )
    .state;
    let mut s2 = diamond_state();
    s2 = take_turn_action(s2, &TurnAction::EndTurn).state;
    s2 = apply(
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
