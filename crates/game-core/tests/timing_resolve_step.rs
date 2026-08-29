//! The triggering condition's own resolution is a step of its timing sequence,
//! and who performs it is an exhaustive classification (#701).
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
//! Driven through `test_support::run_timing_sequence` against a mock registry:
//! the point under test is which cells the coordinator walks for a given
//! classification, so the abilities are mock declarations that mark the cell
//! they fired in rather than real cards. The corpus-level counterpart for
//! `GameEnd` is `cards::tests::cover_up`, which drives the real 01007 through a
//! real scenario ending.

use card_dsl::dsl::{forced_on_event, native, Ability, EventPattern, EventTiming};
use game_core::card_data::CardMetadata;
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::engine::TimingEvent;
use game_core::event::Event;
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, GameState, InvestigatorId, Phase,
};
use game_core::test_support::{run_timing_sequence, test_investigator, GameStateBuilder};
use game_core::{Cx, EngineOutcome, EvalContext};

/// Declares a `when`-timed forced on `PhaseEnded { Upkeep }` — a caller-owned
/// condition, so the coordinator must reject rather than resolve it.
const WHEN_ACT: &str = "TESTWHEN";
/// Declares an `at`-timed forced on `PhaseEnded { Upkeep }` — the cell after the
/// (caller-owned, no-op) resolve step.
const AT_ACT: &str = "TESTAT";
/// Declares `when`- and `at`-timed forceds on `RoundEnded`, a coordinator-owned
/// bare milestone: both cells are walked, in order.
const ROUND_ACT: &str = "TESTROUND";
/// Declares `when`- and `at`-timed forceds on `GameEnd`, the bare milestone #720
/// migrated. Held in an investigator's threat area rather than on the act, since
/// the `GameEnd` forced scan reads the cards each active investigator controls.
const GAME_END_CARD: &str = "TESTGAMEEND";

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
        GAME_END_CARD => Some(vec![
            forced_on_event(
                EventPattern::GameEnd,
                EventTiming::When,
                native("mark:when"),
            ),
            forced_on_event(EventPattern::GameEnd, EventTiming::At, native("mark:at")),
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
        back_abilities_for: |_| None,
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

/// The game's ending is the second bare milestone (#720): its resolve step is a
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
    let inv = InvestigatorId(1);
    let mut investigator = test_investigator(1);
    investigator.threat_area.push(CardInPlay::enter_play(
        CardCode::new(GAME_END_CARD),
        CardInstanceId(1),
    ));
    let mut state = GameStateBuilder::new()
        .with_investigator(investigator)
        .with_turn_order([inv])
        .build();
    let mut events = Vec::new();

    let out = run_timing_sequence(&mut state, &mut events, TimingEvent::GameEnd);

    assert!(
        matches!(out, EngineOutcome::Done),
        "the walk completes rather than rejecting the `when` declaration: {out:?}"
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
        "the `when` cell is walked and precedes the `at` cell, with the no-op resolve step \
         between them leaving the order undisturbed"
    );
}

/// `EmitStep::cells()` is the whole-sequence cell list, derived from the
/// coordinator's own cursor rather than written out beside it (#720). Pinned
/// here because a caller asking a whole-sequence question — elimination's
/// `has_weakness_game_end_ability` — gets a *silent drop* if the list is short,
/// not a reject: it is the one failure mode the fork cannot report.
#[test]
fn the_cell_list_is_every_cell_of_the_sequence_in_order() {
    assert_eq!(
        game_core::state::EmitStep::cells().collect::<Vec<_>>(),
        vec![EventTiming::When, EventTiming::At, EventTiming::After],
        "the three cells, in sequence order, with the resolve step filtered out"
    );
}
