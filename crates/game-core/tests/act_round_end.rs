//! Round-end `when` act-advance window through the public `apply` entry.
//!
//! The Upkeep round-end coordinator opens act 01109's `When`-`RoundEnded`
//! reaction as a board candidate; picking it fires the advance / `Skip`
//! declines.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, OptionTarget, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    Act, CardCode, GameState, GameStateBuilder, InvestigatorId, Location, LocationId,
};
use game_core::test_support::{seat, MockRegistry, TestSession};

const INV: InvestigatorId = InvestigatorId(1);
/// The Hallway, where the investigator stands.
const HERE: LocationId = LocationId(2);

/// The advance logic lives in the registry (01109's `When`-`RoundEnded` reaction
/// native), so the coordinator fires it through the effect evaluator when its
/// candidate is picked. A minimal mock registry stands in for `cards`.
fn advance_native(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.round_end_advance("01112") // the Hallway
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
            .with_abilities("01109", advance_reaction)
            .with_native_effect("test:advance", advance_native)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// Act 2 (01109) current, a Hallway investigator with `clues`, at the end of
/// the Upkeep phase. Act 3 (01110) is the terminal-Won successor.
fn upkeep_round_end_state(clues: u8) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).clues(clues))
        .with_seating([INV])
        .ending_upkeep_phase()
        .with_location(Location::new(
            HERE,
            CardCode("01112".into()),
            "Hallway",
            1,
            0,
        ))
        .build();
    // No builder setter stages the act deck.
    state.act_deck = vec![
        Act {
            code: CardCode("01109".into()),
            clue_threshold: 3,
        },
        Act {
            code: CardCode("01110".into()),
            clue_threshold: 0,
        },
    ];
    state.act_index = 0;
    state
}

/// Open the round-end `when` window: settling runs the Upkeep phase end into
/// the round-end coordinator, which scans act 01109's `When`-`RoundEnded`
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
