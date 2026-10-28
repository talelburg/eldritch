use card_dsl::dsl::{Condition, Effect, ForEachTargets, Trigger, TriggerKind};
use game_core::card_registry::NativeEffectFn;
use game_core::engine::evaluator::EachBinding;
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, CardInPlay, Enemy, GameStateBuilder, InvestigatorId, Location, Owner,
};
use game_core::test_support;

use super::*;
use crate::REGISTRY;

mod ghoul_move;
mod reverse;
mod round_end_doom;

/// Ghoul Minion 01160 (Humanoid. Monster. Ghoul.), not Elite.
fn ghoul(id: u32, at: LocationId) -> Enemy {
    let mut e = test_support::test_enemy(id, "Ghoul");
    e.code = CardCode::new("01160");
    e.current_location = Some(at);
    e
}

// Hallway(2) hub connects to Attic(3), Cellar(4), Parlor(5).
fn star_board() -> GameState {
    star_board_from(GameStateBuilder::new().with_registries(crate::TEST_REGISTRIES))
}

/// The [`star_board`]'s map on top of `builder`.
fn star_board_from(builder: GameStateBuilder) -> GameState {
    let loc =
        |id, code: &str, name| Location::new(LocationId(id), CardCode::new(code), name, 1, 0);
    let mut state = builder
        .with_location(loc(2, "01112", "Hallway"))
        .with_location(loc(3, "01113", "Attic"))
        .with_location(loc(4, "01114", "Cellar"))
        .with_location(loc(5, "01115", "Parlor"))
        .build();
    for spoke in [LocationId(3), LocationId(4), LocationId(5)] {
        state.connect(LocationId(2), spoke);
    }
    state
}

/// Run `f` for investigator 1, assert it finished, and return its events.
fn run_done(state: &mut GameState, f: NativeEffectFn) -> Vec<Event> {
    let (out, events) = test_support::run_native(
        state,
        &REGISTRY,
        f,
        &EvalContext::for_controller(InvestigatorId(1)),
    );
    assert_eq!(out, EngineOutcome::Done);
    events
}
