use card_dsl::dsl::{Condition, Effect, ForEachTargets, Trigger, TriggerKind};
use game_core::card_registry::NativeEffectFn;
use game_core::engine::evaluator::EachBinding;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, GameStateBuilder, InvestigatorId, Owner,
};
use game_core::test_support::{self, Printed, StagedEnemy, StagedLocation};

use super::*;
use crate::REGISTRY;

mod ghoul_move;
mod reverse;
mod round_end_doom;

/// The Hallway, the hub of the [`star_board_from`] map.
const HALL: LocationId = LocationId(2);
const ATTIC: LocationId = LocationId(3);
const CELLAR: LocationId = LocationId(4);
const PARLOR: LocationId = LocationId(5);
/// The [`detour_board`]'s Study.
const STUDY: LocationId = LocationId(6);

/// Ghoul Minion 01160 (Humanoid. Monster. Ghoul.), not Elite, at `at`.
fn ghoul(id: u32, at: LocationId) -> StagedEnemy<Printed> {
    test_support::enemy(id).as_card("01160").at(at)
}

/// The Gathering's printed location `code` as `id`, connected to
/// `connections`, with a Barricade 01038 attached when `barricaded` — so an
/// enemy's compelled step into it is blocked.
fn gathering_location(
    id: LocationId,
    code: &str,
    connections: &[LocationId],
    barricaded: bool,
) -> StagedLocation<Printed> {
    let location = test_support::location(id)
        .as_card(code)
        .connections(connections.to_vec());
    if barricaded {
        // A seat cannot place a location attachment, so its id is named here.
        location.attachments([CardInPlay::enter_play(
            CardCode::new("01038"),
            CardInstanceId(900 + id.0),
            Owner::Investigator(InvestigatorId(1)),
        )])
    } else {
        location
    }
}

/// The Gathering's star map on top of `builder`: the Hallway(2) hub
/// connects to the Attic(3), Cellar(4) and Parlor(5). A Barricade sits at each
/// of `barricaded`.
fn star_board_barricading(builder: GameStateBuilder, barricaded: &[LocationId]) -> GameState {
    let loc = |id, code, connections: &[LocationId]| {
        gathering_location(id, code, connections, barricaded.contains(&id))
    };
    builder
        .with_location(loc(HALL, "01112", &[ATTIC, CELLAR, PARLOR]))
        .with_location(loc(ATTIC, "01113", &[HALL]))
        .with_location(loc(CELLAR, "01114", &[HALL]))
        .with_location(loc(PARLOR, "01115", &[HALL]))
        .build()
}

/// The star map on top of `builder`, with no Barricade.
fn star_board_from(builder: GameStateBuilder) -> GameState {
    star_board_barricading(builder, &[])
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
