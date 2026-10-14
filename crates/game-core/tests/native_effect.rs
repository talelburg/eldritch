//! `Effect::Native` dispatch: a card's `native(tag)` effect resolves
//! through `CardRegistry.native_effect_for` to a host-provided Rust fn.
//! Exercised via the forced-trigger path (the real apply route) since
//! `apply_effect` is `pub(crate)`.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{self, Agenda, CardCode, GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, MockRegistry, TestSession};

const INV: InvestigatorId = InvestigatorId(1);

const AGENDA: &str = "TEST-AGENDA";
const AGENDA_BAD: &str = "TEST-AGENDA-BAD";

/// Forced at end of enemy phase -> the native effect tagged `tag`.
fn forced_native(tag: &'static str) -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::PhaseEnded {
            phase: dsl::Phase::Enemy,
        },
        EventTiming::After,
        dsl::native(tag),
    )]
}

fn set_doom(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.place_doom_on_agenda(7);
    EngineOutcome::Done
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(AGENDA, || forced_native("test:set-doom"))
            .with_abilities(AGENDA_BAD, || forced_native("test:missing"))
            .with_native_effect("test:set-doom", set_doom)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The lead's open turn under the agenda `code`. A filler deck keeps Upkeep
/// 4.4's draw from dealing empty-deck horror once the enemy phase has ended.
fn state_with_agenda(code: &str) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).deck(["filler-card"]))
        .open_turn(INV)
        .build();
    // No builder setter stages the agenda deck.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(code),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state
}

/// End the lead's turn, and with it the Investigation phase, so play runs into
/// the Enemy phase and its end, where the agenda's forced ability fires. Play
/// then goes on through Upkeep into the next round's Mythos phase.
fn end_the_enemy_phase(state: GameState) -> TestSession {
    TestSession::new(state, registries()).take(&TurnAction::EndTurn)
}

#[test]
fn native_effect_runs_via_registry() {
    let session = end_the_enemy_phase(state_with_agenda(AGENDA));
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: state::Phase::Enemy
        }
    );
    assert_eq!(
        session.state().agenda_doom,
        7 + 1,
        "native effect mutated state: its 7 doom, then the next round's Mythos 1.2 doom",
    );
}

#[test]
fn native_effect_rejects_unknown_tag() {
    let session = end_the_enemy_phase(state_with_agenda(AGENDA_BAD));
    session.expect_rejected();
    // The rejection unwinds the whole step back to the turn menu.
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().agenda_doom, 0, "no mutation on reject");
}
