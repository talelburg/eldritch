//! `Condition::Native` dispatch (#592): a card's `native_condition(tag)`
//! predicate resolves through `CardRegistry.native_condition_for` to a
//! host-provided `fn(&GameState, &EvalContext) -> bool`, and gates the
//! surrounding effect on its verdict.
//!
//! Exercised via the forced-trigger path (the real apply route) since
//! `apply_effect` is `pub(crate)` — same shape as `native_effect.rs`.

use card_dsl::dsl::{self, Ability, EventPattern, EventTiming, InvestigatorTarget};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::evaluator::EvalContext;
use game_core::event::Event;
use game_core::state::{self, Agenda, CardCode, GameState, GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, MockRegistry, TestSession};
use game_core::{assert_event, assert_no_event};

const AGENDA: &str = "TEST-AGENDA";
const AGENDA_BAD: &str = "TEST-AGENDA-BAD";
const INV: InvestigatorId = InvestigatorId(1);

/// Forced at end of the enemy phase: gain 2 resources when the native
/// predicate holds, 5 when it does not — two distinct observable branches.
fn gated(tag: &'static str) -> Vec<Ability> {
    vec![dsl::forced_on_event(
        EventPattern::PhaseEnded {
            phase: dsl::Phase::Enemy,
        },
        EventTiming::After,
        dsl::if_else(
            dsl::native_condition(tag),
            dsl::gain_resources(InvestigatorTarget::You, 2),
            dsl::gain_resources(InvestigatorTarget::You, 5),
        ),
    )]
}

/// Reads both arguments the predicate signature provides: board state and the
/// evaluation context's controller.
fn has_doom(state: &GameState, ctx: &EvalContext) -> bool {
    state.agenda_doom > 0 && state.investigators.contains_key(&ctx.controller)
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_abilities(AGENDA, || gated("test:has-doom"))
        .with_abilities(AGENDA_BAD, || gated("test:missing"))
        .with_native_condition("test:has-doom", has_doom)
        .install();
}

/// The lead's open turn under the agenda `code` carrying `doom`. A filler deck
/// keeps Upkeep 4.4's draw from dealing empty-deck horror once the enemy phase
/// has ended.
fn state_with_agenda(code: &str, doom: u8) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_seat(seat(1).deck(["filler-card"]))
        .open_turn(INV)
        .build();
    // No builder setter stages the agenda deck.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new(code),
        doom_threshold: 10,
    }];
    state.agenda_index = 0;
    state.agenda_doom = doom;
    state
}

/// End the lead's turn, and with it the Investigation phase, so play runs into
/// the Enemy phase and its end, where the agenda's forced ability fires. Play
/// then goes on through Upkeep (and its resource) into the next round's Mythos
/// phase, so a branch is read off its `ResourcesGained` event rather than the
/// pool.
fn end_the_enemy_phase(state: GameState) -> TestSession {
    let session = TestSession::new(state).take(&TurnAction::EndTurn);
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: state::Phase::Enemy
        }
    );
    session
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&INV].resources
}

#[test]
fn native_condition_holding_takes_the_then_branch() {
    let session = end_the_enemy_phase(state_with_agenda(AGENDA, 1));
    // Predicate true → `then` (2), not `else_` (5).
    assert_event!(session.events(), Event::ResourcesGained { investigator, amount: 2 } if *investigator == INV);
    assert_no_event!(session.events(), Event::ResourcesGained { amount: 5, .. });
}

#[test]
fn native_condition_failing_takes_the_else_branch() {
    let session = end_the_enemy_phase(state_with_agenda(AGENDA, 0));
    // Predicate false → `else_` (5), not `then` (2).
    assert_event!(session.events(), Event::ResourcesGained { investigator, amount: 5 } if *investigator == INV);
    assert_no_event!(session.events(), Event::ResourcesGained { amount: 2, .. });
}

/// An unregistered tag is a card-authoring bug, not a `false` verdict — it
/// rejects loudly rather than silently taking the `else_` branch.
#[test]
fn native_condition_rejects_unknown_tag() {
    let state = state_with_agenda(AGENDA_BAD, 1);
    let before = resources(&state);
    let session = TestSession::new(state).take(&TurnAction::EndTurn);
    session.expect_rejected();
    // The rejection unwinds the whole step back to the turn menu.
    session.assert_at_turn_menu(INV);
    assert_eq!(resources(session.state()), before, "no mutation on reject");
}
