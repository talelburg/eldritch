//! Regression for #571: a native that writes through an exit and then
//! suspends for a choice is `Rejected` in every build.
//!
//! That is the contract `NativeCx` enforces: a native chooses before it
//! writes. The apply's snapshot rollback undoes the rejected native's write. One
//! that suspends before any write suspends and resumes normally.
//!
//! Each native sits on its own mock act as a forced ability at the end of the
//! enemy phase, the `native_effect.rs` idiom. The natives model the contract,
//! not printed cards (ADR 0016).
//!
//! Its own test binary: the mock registry is built once per process, and the
//! sibling exit tests (`spend_uses.rs`, `native_exits.rs`) build their own.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    self, Act, Agenda, CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};

const WRITE_THEN_SUSPEND_ACT: &str = "_ncx_act_write_then_suspend";
const SUSPEND_THEN_WRITE_ACT: &str = "_ncx_act_suspend_then_write";

const WRITE_THEN_SUSPEND: &str = "test:write-then-suspend";

/// The two locations the natives offer as a choice.
const NEAR: LocationId = LocationId(10);
const FAR: LocationId = LocationId(11);

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

/// Offer the controller a pick between [`NEAR`] and [`FAR`].
fn pick_location(cx: &NativeCx, ctx: &EvalContext) -> Grounded<LocationId> {
    engine::resolve_grounded_choice(
        cx.state(),
        cx.cards(),
        ctx,
        &[NEAR, FAR],
        "Choose a location",
        |&loc| OptionTarget::Location(loc),
    )
}

/// The contract violation: place doom, *then* ask for a location.
fn write_then_suspend(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    cx.place_doom_on_agenda(3);
    match pick_location(cx, ctx) {
        Grounded::Picked(loc) => {
            cx.place_investigator(ctx.controller, loc);
            EngineOutcome::Done
        }
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(outcome) => outcome,
    }
}

/// The contract kept: ask for a location, then place doom and the
/// investigator there.
fn suspend_then_write(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    match pick_location(cx, ctx) {
        Grounded::Picked(loc) => {
            cx.place_doom_on_agenda(3);
            cx.place_investigator(ctx.controller, loc);
            EngineOutcome::Done
        }
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(outcome) => outcome,
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(WRITE_THEN_SUSPEND_ACT, || forced_native(WRITE_THEN_SUSPEND))
            .with_native_effect(WRITE_THEN_SUSPEND, write_then_suspend)
            .with_abilities(SUSPEND_THEN_WRITE_ACT, || {
                forced_native("test:suspend-then-write")
            })
            .with_native_effect("test:suspend-then-write", suspend_then_write)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
/// The investigator's starting location, the third on the board.
const HERE: LocationId = LocationId(1);

/// The lead's open turn at a third location, [`NEAR`] and [`FAR`] on the
/// board, an agenda in play, and `act` as the current act. A filler deck keeps
/// Upkeep 4.4's draw from dealing empty-deck horror once the enemy phase has
/// ended.
fn state_with_act(act: &str) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).deck(["filler-card"]))
        .with_location(test_support::test_location(HERE.0, "Start"))
        .with_location(test_support::test_location(NEAR.0, "Near"))
        .with_location(test_support::test_location(FAR.0, "Far"))
        .open_turn(INV)
        .build();
    // No builder setter stages the act and agenda decks.
    state.act_deck = vec![Act {
        code: CardCode::new(act),
        clue_threshold: 99,
    }];
    state.act_index = 0;
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("TEST-AGENDA"),
        doom_threshold: 10,
    }];
    state
}

/// End the lead's turn, and with it the Investigation phase, so play runs into
/// the Enemy phase and its end, where the act's forced native fires.
fn end_enemy_phase(session: TestSession) -> TestSession {
    session.take(&TurnAction::EndTurn)
}

#[test]
fn a_native_that_writes_then_suspends_is_rejected_and_its_write_rolled_back() {
    let session = TestSession::new(state_with_act(WRITE_THEN_SUSPEND_ACT), registries());
    let before = session.state().clone();

    let session = end_enemy_phase(session);

    let reason = session.expect_rejected();
    assert!(
        reason.contains(WRITE_THEN_SUSPEND),
        "the rejection names the native: {reason}"
    );
    assert_eq!(
        session.state(),
        &before,
        "the doom the native placed before suspending is rolled back"
    );
}

#[test]
fn a_native_that_suspends_before_writing_suspends_and_resumes() {
    let session = end_enemy_phase(TestSession::new(
        state_with_act(SUSPEND_THEN_WRITE_ACT),
        registries(),
    ));
    assert_eq!(session.prompt().options.len(), 2, "both locations offered");
    assert_eq!(session.state().agenda_doom, 0, "nothing written yet");

    let session = session.pick(OptionTarget::Location(FAR));

    // The enemy phase ends and play goes on into the next round's Mythos
    // phase, whose step 1.2 places 1 doom of its own.
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: state::Phase::Enemy
        }
    );
    assert_eq!(session.state().agenda_doom, 3 + 1);
    assert_eq!(
        session.state().investigators[&INV].current_location,
        Some(FAR)
    );
}
