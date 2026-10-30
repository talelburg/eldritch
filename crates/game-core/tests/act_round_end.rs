//! Round-end `when` act-advance window through the public `apply` entry.
//!
//! The Upkeep round-end coordinator opens the current act's `When`-`RoundEnded`
//! reaction as a board candidate; picking it fires the advance / `Skip`
//! declines.
//!
//! Synthetic probes only (ADR 0016): [`ACT_2`] models "an act whose
//! investigators at one location may, as a group, spend its clue threshold at
//! round end to advance", [`ACT_3`] its
//! successor, and [`HALLWAY`] the location that names the contributors.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, OptionTarget, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{act, location, seat, MockRegistry, TestSession};

const INV: InvestigatorId = InvestigatorId(1);
/// The location where the investigator stands.
const HERE: LocationId = LocationId(2);
/// The act current at round end, clue threshold 3, carrying the reaction.
const ACT_2: &str = "_round_end_act";
/// The act after [`ACT_2`].
const ACT_3: &str = "_round_end_next_act";
/// The code of [`HERE`]: investigators there contribute the clues.
const HALLWAY: &str = "_round_end_hallway";

/// The advance logic lives in the registry ([`ACT_2`]'s `When`-`RoundEnded`
/// reaction native), so the coordinator fires it through the effect evaluator
/// when its candidate is picked. A minimal mock registry stands in for `cards`.
fn advance_native(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.round_end_advance(HALLWAY)
}

fn advance_reaction() -> Vec<Ability> {
    vec![dsl::reaction_on_event(
        EventPattern::RoundEnded,
        EventTiming::When,
        dsl::native("test:advance"),
    )]
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(ACT_2, advance_reaction)
            .with_native_effect("test:advance", advance_native)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// [`ACT_2`] current, an investigator with `clues` at [`HALLWAY`], at the end
/// of the Upkeep phase. [`ACT_3`] is the successor.
fn upkeep_round_end_state(clues: u8) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).clues(clues))
        .with_seating([INV])
        .ending_upkeep_phase()
        .with_location(location(HERE).code(HALLWAY))
        .with_act_deck([act(ACT_2, 3), act(ACT_3, 0)])
        .build()
}

/// Open the round-end `when` window: settling runs the Upkeep phase end into
/// the round-end coordinator, which scans [`ACT_2`]'s `When`-`RoundEnded`
/// reaction and suspends on it.
fn opened_round_end_window(clues: u8) -> TestSession {
    let session = TestSession::new(upkeep_round_end_state(clues), registries());
    let offered: Vec<_> = session
        .prompt()
        .options
        .iter()
        .map(|o| o.target.clone())
        .collect();
    assert_eq!(
        offered,
        vec![Some(OptionTarget::Act)],
        "the round-end `when` window offers the act-advance reaction alone",
    );
    session
}

#[test]
fn resolve_pick_fires_advance() {
    // Picking the advance continues the round-end + upkeep cascade into Mythos,
    // which pauses at the step-1.4 encounter-draw prompt.
    let session = opened_round_end_window(3).pick(OptionTarget::Act);
    assert_eq!(session.state().act_index, 1, "advanced act 2 -> act 3");
    assert_eq!(session.state().investigators[&INV].clues, 0, "spent 3");
}

#[test]
fn resolve_skip_declines_advance() {
    let session = opened_round_end_window(3).skip();
    assert_eq!(session.state().act_index, 0, "no advance on Skip");
    assert_eq!(
        session.state().investigators[&INV].clues,
        3,
        "no clues spent on Skip"
    );
}
