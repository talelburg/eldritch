//! The registries are a value handed to each `apply`, not a process global:
//! the same state and action under two different [`Registries`] give two
//! different outcomes, in one test binary (#1032).
//!
//! Whatever the engine reads, it reads from the value it was handed.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::card_registry::{CardRegistry, NativeEffectFn};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, NativeCx, OptionId, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{Agenda, CardCode, GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, TEST_REGISTRY};

const INV: InvestigatorId = InvestigatorId(1);
const AGENDA: &str = "TEST-AGENDA";

fn set_doom(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.place_doom_on_agenda(7);
    EngineOutcome::Done
}

/// The agenda's abilities: forced, at the end of the enemy phase, place 7 doom.
fn abilities_knowing_the_agenda(code: &CardCode) -> Option<Vec<Ability>> {
    if code.as_str() == AGENDA {
        return Some(vec![dsl::forced_on_event(
            EventPattern::PhaseEnded {
                phase: dsl::Phase::Enemy,
            },
            EventTiming::After,
            dsl::native("test:set-doom"),
        )]);
    }
    (TEST_REGISTRY.abilities_for)(code)
}

fn native_set_doom(tag: &str) -> Option<NativeEffectFn> {
    (tag == "test:set-doom").then_some(set_doom as NativeEffectFn)
}

/// game-core's test registry, plus an agenda with a forced doom ability.
const KNOWS_THE_AGENDA: CardRegistry = CardRegistry {
    abilities_for: abilities_knowing_the_agenda,
    native_effect_for: native_set_doom,
    ..TEST_REGISTRY
};

fn registries(cards: &'static CardRegistry) -> Registries {
    Registries {
        cards,
        scenarios: &ScenarioRegistry::EMPTY,
    }
}

/// The lead's open turn under the agenda. A filler deck keeps Upkeep's draw
/// from dealing empty-deck horror.
fn state() -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(registries(&TEST_REGISTRY))
        .with_seat(seat(1).deck(["filler-card"]))
        .open_turn(INV)
        .build();
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(AGENDA),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state
}

/// End the lead's turn — the only investigator, so the Investigation phase
/// ends and play runs through the Enemy phase's end.
fn end_turn(state: GameState, regs: Registries) -> GameState {
    let idx = enumerate::legal_actions(&state, regs.cards)
        .iter()
        .position(|a| *a == TurnAction::EndTurn)
        .expect("EndTurn is on the turn menu");
    let action = Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
    });
    let result = engine::apply(state, action, regs);
    assert!(
        !matches!(result.outcome, EngineOutcome::Rejected { .. }),
        "EndTurn rejected: {:?}",
        result.outcome
    );
    result.state
}

#[test]
fn the_same_action_under_two_registries_gives_two_outcomes() {
    let with_ability = end_turn(state(), registries(&KNOWS_THE_AGENDA));
    let without = end_turn(state(), registries(&TEST_REGISTRY));
    assert_eq!(
        with_ability.agenda_doom,
        7 + 1,
        "the registry that knows the agenda fires its forced ability: its 7 doom, \
         then the next round's Mythos doom",
    );
    assert_eq!(
        without.agenda_doom, 1,
        "the registry that doesn't know it places only the Mythos doom",
    );
}
