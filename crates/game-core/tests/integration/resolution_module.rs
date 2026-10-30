//! The scenario module's `apply_resolution` hook, run when a terminal act's
//! reverse latches a resolution.
//!
//! These tests need a scenario module that stamps the state when its hook
//! runs, so this file names its own registries: the test card registry
//! beside that module (prior art: `tests/scenario_resolution.rs`).

use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{legal_actions, TurnAction};
use game_core::engine::{EngineOutcome, OptionId, Registries};
use game_core::event::Event;
use game_core::scenario::{
    ResolutionId, ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry,
};
use game_core::state::{Act, GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support::{self, seat, TestSession, TEST_REGISTRY};
use game_core::{assert_event, assert_no_event};

/// The scenario id the stamp module answers to.
const STAMP: &str = "stamp";

/// The one seated investigator, whose turn is open.
const INV: InvestigatorId = InvestigatorId(1);

const ADVANCE_ACT: TurnAction = TurnAction::AdvanceAct { investigator: INV };

/// The resources the stamp module sets, so a test can tell its hook ran.
const STAMPED: u8 = 99;

/// [`TEST_REGISTRY`] beside the stamp module. The test registry serves the
/// reverse of the terminal act card: without it the advance would reach no
/// ending (ADR 0013).
const REGISTRIES: Registries = Registries {
    cards: &TEST_REGISTRY,
    scenarios: &ScenarioRegistry {
        module_for: stamp_module_for,
    },
};

/// `apply_resolution` that records it ran by stamping the acting
/// investigator's resources to [`STAMPED`], so tests can assert the module
/// hook (not just the event) fired.
fn stamp_apply(_ending: ScenarioEnding, state: &mut GameState, _events: &mut Vec<Event>) {
    if let Some(inv) = state.investigators.values_mut().next() {
        inv.resources = STAMPED;
    }
}

fn unused_setup() -> GameState {
    GameStateBuilder::new().build()
}

static STAMP_MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: None,
    setup: unused_setup,
    apply_resolution: stamp_apply,
    layout: &[],
};

fn stamp_module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == STAMP).then_some(&STAMP_MODULE)
}

/// The stamp scenario at `INV`'s turn menu in the Investigation phase, whose
/// current (only) act is terminal — it is the only card in the deck
/// (ADR 0013) — and whose investigator holds `clues` clues: one is exactly
/// enough to advance it. A single `AdvanceAct` flips it, and its reverse
/// latches `Resolution(1)`.
fn terminal_act_state(clues: u8) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(REGISTRIES)
        .with_seat(seat(1).clues(clues))
        .open_turn(INV)
        .with_scenario_id(ScenarioId::new(STAMP))
        .build();
    // No builder setter for the act deck.
    state.act_deck = vec![Act {
        code: test_support::terminal_code(1),
        clue_threshold: 1,
    }];
    state
}

/// The stamp scenario after `INV` advances its terminal act.
fn advanced() -> TestSession {
    TestSession::new(terminal_act_state(1), REGISTRIES).take(&ADVANCE_ACT)
}

#[test]
fn resolution_fires_and_applies_when_latch_set_with_module() {
    let result = advanced().finish();
    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "the scenario has ended"
    );
    assert_event!(
        result.events,
        Event::ScenarioResolved {
            ending: ScenarioEnding::Resolution(id)
        } if *id == ResolutionId::new(1)
    );
    assert_eq!(
        result.state.investigators[&INV].resources, STAMPED,
        "apply_resolution ran"
    );
}

/// Regression for #566: a resolution cancels the framework sequence it was
/// suspended in, so the engine stops offering the turn menu.
#[test]
fn resolution_cancels_the_open_turn_and_does_not_refire() {
    // `terminal_act_state` opens the turn: AdvanceAct latches the terminal
    // act's resolution.
    let session = advanced();
    assert_event!(session.events(), Event::ScenarioResolved { .. });
    // The scenario ended, so the framework sequence it was suspended in is
    // cancelled: the open turn and its phase anchor are gone, and with them
    // every legal action, so nothing cascades into the next round's Mythos
    // phase. A stack invariant: no frame survives the ending.
    assert!(
        session.state().continuations.is_empty(),
        "the open turn and phase anchor are cancelled: {:?}",
        session.state().continuations,
    );
    assert!(legal_actions(session.state(), REGISTRIES.cards).is_empty());

    // A later answer finds no prompt outstanding and rejects; the
    // already-finished resolution does not re-fire. The subject is that
    // stray wire action itself, so it is sent raw.
    let second = session
        .apply(Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }))
        .finish();
    assert!(matches!(second.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(second.events, Event::ScenarioResolved { .. });
}

#[test]
fn resolution_skipped_on_rejected_outcome() {
    let session = TestSession::new(terminal_act_state(0), REGISTRIES);
    // With 0 clues AdvanceAct is illegal, so it is not enumerated — there is
    // no OptionId to submit (OptionId routing expresses illegality as
    // non-enumeration).
    let legal = legal_actions(session.state(), REGISTRIES.cards);
    assert!(!legal
        .iter()
        .any(|a| matches!(a, TurnAction::AdvanceAct { .. })));
    // Submitting an out-of-range OptionId rejects, and a Rejected outcome must
    // skip the resolution hook (no ScenarioResolved fires). The subject is
    // that malformed wire action itself, so it is sent raw.
    let session = session.apply(Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(u32::try_from(legal.len()).unwrap())),
    }));
    session.expect_rejected();
    assert_no_event!(session.finish().events, Event::ScenarioResolved { .. });
}

#[test]
fn resolution_places_no_victory_without_qualifying_locations() {
    // No victory-bearing locations in play → nothing placed, no event,
    // no panic (covers the registry-absent / no-location path).
    let result = advanced().finish();
    assert!(result.state.victory_display.is_empty());
    assert_no_event!(result.events, Event::EnteredVictoryDisplay { .. });
}
