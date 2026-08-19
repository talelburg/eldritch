//! The triggering condition's own resolution is a step of its timing sequence,
//! and who performs it is an exhaustive classification (#701).
//!
//! `glossary/Nested_Sequences.md` puts the condition's resolution *inside* the
//! sequence: *"Each time a triggering condition occurs, the following sequence
//! is followed: 1) execute "when..." effects that interrupt that triggering
//! condition, (2) resolve the triggering condition, and then, (3) execute
//! "after..." effects in response to that triggering condition."*
//!
//! Two arms. A **coordinator-owned** condition (today: only `RoundEnded`, a bare
//! milestone with nothing to resolve) walks its `when` cell. A **caller-owned**
//! condition has already been resolved by its emitting call site, so its `when`
//! cell is not walked and an ability declaring interrupt timing on it is
//! rejected rather than silently dropped.
//!
//! No corpus card declares interrupt timing on a caller-owned condition, and
//! `queue_event` routes only `RoundEnded` through the coordinator until #702, so
//! this walks the cells through `test_support::run_timing_sequence` against a
//! mock registry.

use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::engine::TimingEvent;
use game_core::event::Event;
use game_core::state::{Act, CardCode, GameState, InvestigatorId, Phase};
use game_core::test_support::{run_timing_sequence, test_investigator, GameStateBuilder};
use game_core::{Cx, EngineOutcome, EvalContext};

/// Declares a `when`-timed forced on `PhaseEnded { Upkeep }` — a caller-owned
/// condition, so the coordinator must reject rather than resolve it.
const WHEN_ACT: &str = "TESTWHEN";
/// Declares an `at`-timed forced on `PhaseEnded { Upkeep }` — the cell after the
/// (caller-owned, no-op) resolve step.
const AT_ACT: &str = "TESTAT";
/// Declares `when`- and `at`-timed forceds on `RoundEnded`, the one
/// coordinator-owned condition: both cells are walked, in order.
const ROUND_ACT: &str = "TESTROUND";

/// A cell marker: one `ResourcesGained` whose amount names the cell that fired
/// it, so a test can assert *which* cells ran and *in what order*.
fn mark(cx: &mut Cx, ctx: &EvalContext, amount: u8) -> EngineOutcome {
    cx.events.push(Event::ResourcesGained {
        investigator: ctx.controller,
        amount,
    });
    EngineOutcome::Done
}

fn mark_when(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    mark(cx, ctx, 1)
}

fn mark_at(cx: &mut Cx, ctx: &EvalContext) -> EngineOutcome {
    mark(cx, ctx, 2)
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        WHEN_ACT => Some(vec![forced_on_event(
            EventPattern::PhaseEnded {
                phase: card_dsl::dsl::Phase::Upkeep,
            },
            EventTiming::When,
            native("mark:when"),
        )]),
        AT_ACT => Some(vec![forced_on_event(
            EventPattern::PhaseEnded {
                phase: card_dsl::dsl::Phase::Upkeep,
            },
            EventTiming::At,
            native("mark:at"),
        )]),
        ROUND_ACT => Some(vec![
            forced_on_event(
                EventPattern::RoundEnded,
                EventTiming::When,
                native("mark:when"),
            ),
            forced_on_event(EventPattern::RoundEnded, EventTiming::At, native("mark:at")),
        ]),
        _ => None,
    }
}

fn mock_native_for(tag: &str) -> Option<NativeEffectFn> {
    match tag {
        "mark:when" => Some(mark_when as NativeEffectFn),
        "mark:at" => Some(mark_at as NativeEffectFn),
        _ => None,
    }
}

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: mock_native_for,
        native_eligibility_for: |_| None,
        native_condition_for: |_| None,
    });
}

/// One investigator, one act — `act` is the card whose declared timing is on
/// trial (the `PhaseEnded` / `RoundEnded` forced scans read the current act).
fn state_with_act(act: &str) -> GameState {
    let inv = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_phase(Phase::Upkeep)
        .with_investigator(test_investigator(1))
        .with_turn_order([inv])
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(act),
        clue_threshold: 0,
        resolution: None,
    }];
    state.act_index = 0;
    state
}

#[test]
fn caller_owned_condition_rejects_a_declared_interrupt() {
    let mut state = state_with_act(WHEN_ACT);
    let mut events = Vec::new();
    let out = run_timing_sequence(
        &mut state,
        &mut events,
        TimingEvent::PhaseEnded {
            phase: Phase::Upkeep,
        },
    );
    let EngineOutcome::Rejected { reason } = out else {
        panic!("a `when`-timed ability on a caller-owned condition must reject, got {out:?}");
    };
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
    let mut state = state_with_act(AT_ACT);
    let mut events = Vec::new();
    let out = run_timing_sequence(
        &mut state,
        &mut events,
        TimingEvent::PhaseEnded {
            phase: Phase::Upkeep,
        },
    );
    assert!(
        matches!(out, EngineOutcome::Done),
        "walk completes: {out:?}"
    );
    assert!(
        events.contains(&Event::ResourcesGained {
            investigator: InvestigatorId(1),
            amount: 2,
        }),
        "the `at` cell fires past the skipped `when` cell and the no-op resolve step: {events:?}"
    );
}

#[test]
fn coordinator_owned_condition_walks_its_when_cell_before_its_at_cell() {
    let mut state = state_with_act(ROUND_ACT);
    let mut events = Vec::new();
    let out = run_timing_sequence(&mut state, &mut events, TimingEvent::RoundEnded);
    assert!(
        matches!(out, EngineOutcome::Done),
        "walk completes: {out:?}"
    );
    let cells: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(
        cells,
        vec![1, 2],
        "the round end is coordinator-owned: its `when` cell is walked, and the resolve step \
         between the cells does not disturb the order"
    );
}
