//! The triggering condition's own resolution is a step of its timing sequence,
//! and who performs it is an exhaustive classification.
//!
//! `glossary/Nested_Sequences.md` puts the condition's resolution *inside* the
//! sequence: *"Each time a triggering condition occurs, the following sequence
//! is followed: 1) execute "when..." effects that interrupt that triggering
//! condition, (2) resolve the triggering condition, and then, (3) execute
//! "after..." effects in response to that triggering condition."*
//!
//! Two arms. A **coordinator-owned** condition walks its `when` cell; the two
//! walked here are `RoundEnded` and `GameEnd`, both **bare milestones** with
//! nothing to resolve between the cells. A **caller-owned** condition has
//! already been resolved by its emitting call site, so its `when` cell is not
//! walked and an ability declaring interrupt timing on it is rejected rather
//! than silently dropped.
//!
//! Driven through the real moments — the Upkeep phase's end and a terminal
//! act's advance — against a mock registry: the point under test is which cells the coordinator walks for a given
//! classification, so the abilities are mock declarations that mark the cell
//! they fired in rather than real cards. The corpus-level counterpart for
//! `GameEnd` is `cards::tests::cover_up`, which drives the real 01007 through a
//! real scenario ending.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Ability, Effect, EventPattern, EventTiming, InvestigatorTarget};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{ApplyResult, EngineOutcome, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    self, Act, CardCode, EmitStep, GameState, GameStateBuilder, InvestigatorId,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};

/// Declares a `when`-timed forced on `PhaseEnded { Upkeep }` — a caller-owned
/// condition, so the coordinator must reject rather than resolve it.
const WHEN_ACT: &str = "TESTWHEN";
/// Declares an `at`-timed forced on `PhaseEnded { Upkeep }` — the cell after the
/// (caller-owned, no-op) resolve step.
const AT_ACT: &str = "TESTAT";
/// Declares `when`- and `at`-timed forceds on `RoundEnded`, a coordinator-owned
/// bare milestone: both cells are walked, in order.
const ROUND_ACT: &str = "TESTROUND";
/// Declares `when`- and `at`-timed forceds on `GameEnd`, a coordinator-owned
/// bare milestone. Held in an investigator's threat area rather than on the act, since
/// the `GameEnd` forced scan reads the cards each active investigator controls.
const GAME_END_CARD: &str = "TESTGAMEEND";

/// A cell marker: one `ResourcesGained` whose amount names the cell that fired
/// it, so a test can assert *which* cells ran and *in what order*.
fn mark(amount: u8) -> Effect {
    dsl::gain_resources(InvestigatorTarget::You, amount)
}

/// The `when` cell's marker.
fn mark_when() -> Effect {
    mark(1)
}

/// The `at` cell's marker.
fn mark_at() -> Effect {
    mark(2)
}

/// The `when`/`at` marker pair a bare milestone's card declares, so both cells
/// are observable in order.
fn both_cells(pattern: EventPattern) -> Vec<Ability> {
    vec![
        dsl::forced_on_event(pattern.clone(), EventTiming::When, mark_when()),
        dsl::forced_on_event(pattern, EventTiming::At, mark_at()),
    ]
}

fn upkeep_ended() -> EventPattern {
    EventPattern::PhaseEnded {
        phase: dsl::Phase::Upkeep,
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(WHEN_ACT, || {
                vec![dsl::forced_on_event(
                    upkeep_ended(),
                    EventTiming::When,
                    mark_when(),
                )]
            })
            .with_abilities(AT_ACT, || {
                vec![dsl::forced_on_event(
                    upkeep_ended(),
                    EventTiming::At,
                    mark_at(),
                )]
            })
            .with_abilities(ROUND_ACT, || both_cells(EventPattern::RoundEnded))
            .with_abilities(GAME_END_CARD, || both_cells(EventPattern::GameEnd))
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);

/// The investigator, alone.
fn lead_alone() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(INV.0))
        .with_turn_order([INV])
}

/// Build `board` with `act` current — the card whose declared timing is on
/// trial (the `PhaseEnded` / `RoundEnded` forced scans read the current act).
/// The act deck has no builder setter, so it is set on the built state.
fn with_act(board: GameStateBuilder, act: impl Into<CardCode>, clue_threshold: u8) -> GameState {
    let mut state = board.build();
    state.act_deck = vec![Act {
        code: act.into(),
        clue_threshold,
    }];
    state.act_index = 0;
    state
}

/// The end of the Upkeep phase, and with it the round, staged with
/// [`ending_upkeep_phase`](GameStateBuilder::ending_upkeep_phase) under `act`.
/// Settling runs step 4.6 — `PhaseEnded { Upkeep }`, then `RoundEnded` — and
/// play goes on into the next round's Mythos phase; settling would panic had
/// the walk been rejected.
fn end_the_upkeep_phase(act: &str) -> TestSession {
    let session = TestSession::new(
        with_act(lead_alone().ending_upkeep_phase(), act, 0),
        registries(),
    );
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: state::Phase::Upkeep
        }
    );
    session
}

/// The cell markers that fired, in order.
fn cells(events: &[Event]) -> Vec<u8> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect()
}

#[test]
fn caller_owned_condition_rejects_a_declared_interrupt() {
    // A rejected phase end cannot be settled into a session, so the Upkeep
    // phase end is reached by the step that really gets there: ending the
    // last turn runs the Investigation, Enemy and Upkeep phases to step 4.6.
    let session = TestSession::new(
        with_act(lead_alone().open_turn(INV), WHEN_ACT, 0),
        registries(),
    )
    .take(&TurnAction::EndTurn);
    let reason = session.expect_rejected();
    assert!(
        reason.contains("caller-owned"),
        "the reject names the classification: {reason}"
    );
    assert!(
        reason.contains("PhaseEnded"),
        "the reject names the condition: {reason}"
    );
    assert!(
        reason.contains("0008-a-triggering-condition-resolves-inside-its-own-sequence"),
        "the reject names the ADR: {reason}"
    );
    assert!(
        reason.contains("#704"),
        "the reject names the migration that removes it: {reason}"
    );
}

#[test]
fn caller_owned_condition_walks_the_cells_after_its_resolve_step() {
    let session = end_the_upkeep_phase(AT_ACT);
    assert!(
        session.events().contains(&Event::ResourcesGained {
            investigator: INV,
            amount: 2,
        }),
        "the `at` cell fires past the skipped `when` cell and the no-op resolve step: {:?}",
        session.events(),
    );
}

#[test]
fn coordinator_owned_condition_walks_its_when_cell_before_its_at_cell() {
    let session = end_the_upkeep_phase(ROUND_ACT);
    assert_eq!(
        cells(session.events()),
        vec![1, 2],
        "the round end is coordinator-owned: its `when` cell is walked, and the resolve step \
         between the cells does not disturb the order"
    );
}

/// The game's ending is the second bare milestone: its resolve step is a
/// documented no-op, so the `when` cell is walked and an ability declaring it
/// resolves *before* the `at` cell rather than being rejected.
///
/// Cover Up 01007's *"**Forced** - When the game ends, if there are any clues on
/// Cover Up: You suffer 1 mental trauma."* is the card that demanded it. What
/// makes the milestone bare is that the ending's own teardown — the
/// victory-display scan and `apply_resolution` — is not this condition's impact:
/// the `ScenarioEnd` frame emits in tail position and finalizes at the apply
/// boundary, after the whole sequence has drained.
#[test]
fn the_game_ending_is_a_bare_milestone_that_walks_its_when_cell() {
    // The game really ends: the investigator spends a clue to advance the
    // terminal act, whose reverse reaches a resolution. `GAME_END_CARD` is a
    // code the registry doesn't know, so the encounter deck owns it.
    let board = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(INV.0).clues(1).threat_area([GAME_END_CARD]))
        .with_turn_order([INV])
        .open_turn(INV);
    let session = TestSession::new(
        with_act(board, test_support::terminal_code(1), 1),
        registries(),
    )
    .take(&TurnAction::AdvanceAct { investigator: INV });

    assert!(
        session.state().ending.is_some(),
        "the terminal act ended the game"
    );
    let ApplyResult {
        events,
        outcome: out,
        ..
    } = session.finish();
    assert!(
        matches!(out, EngineOutcome::Done),
        "the walk completes rather than rejecting the `when` declaration: {out:?}"
    );
    assert_eq!(
        cells(&events),
        vec![1, 2],
        "the `when` cell is walked and precedes the `at` cell, with the no-op resolve step \
         between them leaving the order undisturbed"
    );
}

/// `EmitStep::cells()` is the whole-sequence cell list, derived from the
/// coordinator's own cursor rather than written out beside it. Pinned
/// here because a caller asking a whole-sequence question — elimination's
/// `has_weakness_game_end_ability` — gets a *silent drop* if the list is short,
/// not a reject: it is the one failure mode the fork cannot report.
#[test]
fn the_cell_list_is_every_cell_of_the_sequence_in_order() {
    assert_eq!(
        EmitStep::cells().collect::<Vec<_>>(),
        vec![EventTiming::When, EventTiming::At, EventTiming::After],
        "the three cells, in sequence order, with the resolve step filtered out"
    );
}
