//! `TestSession::fire_at` and the framework steps (#946) over the real engine.
//!
//! A timing point fired from a test runs the way it does in play: the
//! coordinator walks its cells, each cell resolves its forced abilities before
//! its reactions, two simultaneous forced abilities become the lead's ordering
//! run, and an ending latched along the way is finalized. A phase the builder
//! stages at its end runs that phase end when the session settles. Every test drives
//! through the session's public steps and asserts on the prompt, the events and
//! the board. The one read of the continuation stack is the ending test's: a
//! finished game leaves nothing to resolve.
//!
//! Synthetic probes only (ADR 0016). Each `_fa_*` code models one primitive —
//! a forced ability at the end of the round that marks which ability fired, a
//! reaction at the same timing point, a forced ability at a phase end that
//! marks the same way, a forced ability that reaches a
//! resolution point — and none stands in for a printed card. They live here,
//! the one binary that reads them.

use card_dsl::dsl::{self, EventPattern, EventTiming};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{Cx, EngineOutcome, InputKind, OptionTarget, TimingEvent};
use game_core::event::Event;
use game_core::scenario::{ResolutionId, ScenarioEnding};
use game_core::state::{
    Act, CardCode, CardInPlay, CardInstanceId, GameStateBuilder, Investigator, InvestigatorId,
    Phase,
};
use game_core::test_support::{self, MockRegistry, TestSession};
use game_core::{assert_event, assert_event_sequence};

/// `Forced - At the end of the round: mark 1.`
const FORCED_A: &str = "_fa_forced_a";
/// `Forced - At the end of the round: mark 2.` A second, simultaneous forced
/// ability at the same timing point as [`FORCED_A`].
const FORCED_B: &str = "_fa_forced_b";
/// `[reaction] At the end of the round: mark 3.`
const REACTION: &str = "_fa_reaction";
/// `Forced - At the end of the enemy phase: mark 4.` An act ability, since
/// the phase-end scan reads the current act and agenda.
const PHASE_END: &str = "_fa_phase_end";
/// `Forced - At the end of the upkeep phase: mark 5.` An act ability, like
/// [`PHASE_END`].
const UPKEEP_END: &str = "_fa_upkeep_end";
/// `Forced - At the end of the round: (→R1)` — reaches a resolution point.
const RESOLVES: &str = "_fa_resolves";

const INV: InvestigatorId = InvestigatorId(1);

/// A marker: one `ResourcesGained` whose amount names the ability that fired,
/// so a test can read which abilities ran and in what order.
fn mark(cx: &mut Cx, ctx: &EvalContext, amount: u8) -> EngineOutcome {
    cx.events.push(Event::ResourcesGained {
        investigator: ctx.controller,
        amount,
    });
    EngineOutcome::Done
}

#[ctor::ctor(unsafe)]
fn install() {
    MockRegistry::new()
        .with_abilities(FORCED_A, || {
            vec![dsl::forced_on_event(
                EventPattern::RoundEnded,
                EventTiming::At,
                dsl::native("_fa:mark1"),
            )]
        })
        .with_abilities(FORCED_B, || {
            vec![dsl::forced_on_event(
                EventPattern::RoundEnded,
                EventTiming::At,
                dsl::native("_fa:mark2"),
            )]
        })
        .with_abilities(REACTION, || {
            vec![dsl::reaction_on_event(
                EventPattern::RoundEnded,
                EventTiming::At,
                dsl::native("_fa:mark3"),
            )]
        })
        .with_abilities(RESOLVES, || {
            vec![dsl::forced_on_event(
                EventPattern::RoundEnded,
                EventTiming::At,
                dsl::reach_resolution(1),
            )]
        })
        .with_abilities(PHASE_END, || {
            vec![dsl::forced_on_event(
                EventPattern::PhaseEnded {
                    phase: dsl::Phase::Enemy,
                },
                EventTiming::At,
                dsl::native("_fa:mark4"),
            )]
        })
        .with_abilities(UPKEEP_END, || {
            vec![dsl::forced_on_event(
                EventPattern::PhaseEnded {
                    phase: dsl::Phase::Upkeep,
                },
                EventTiming::At,
                dsl::native("_fa:mark5"),
            )]
        })
        .with_native_effect("_fa:mark4", |cx, ctx| mark(cx, ctx, 4))
        .with_native_effect("_fa:mark5", |cx, ctx| mark(cx, ctx, 5))
        .with_native_effect("_fa:mark1", |cx, ctx| mark(cx, ctx, 1))
        .with_native_effect("_fa:mark2", |cx, ctx| mark(cx, ctx, 2))
        .with_native_effect("_fa:mark3", |cx, ctx| mark(cx, ctx, 3))
        .install();
}

/// The investigator, holding `codes` in their threat area, instance ids from 1
/// in order — the round-end scans read the cards each active investigator
/// controls.
fn holding(codes: &[&str]) -> Investigator {
    let mut investigator = test_support::test_investigator(1);
    for (i, code) in codes.iter().enumerate() {
        investigator
            .threat_area
            .push(CardInPlay::enter_play(CardCode::new(*code), instance(i)));
    }
    investigator
}

/// One investigator holding `codes`, settled.
fn session_holding(codes: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_investigator(holding(codes))
        .with_turn_order([INV])
        .session()
}

fn instance(i: usize) -> CardInstanceId {
    CardInstanceId(u32::try_from(i + 1).expect("small index"))
}

/// The markers fired so far, in order.
fn marks(session: &TestSession) -> Vec<u8> {
    session
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect()
}

#[test]
fn a_reaction_is_offered_after_the_forced_ability_at_the_same_timing_point() {
    let session = session_holding(&[FORCED_A, REACTION]).fire_at(TimingEvent::RoundEnded);

    assert_eq!(
        marks(&session),
        vec![1],
        "the forced ability resolved before the reaction was offered"
    );
    let prompt = session.prompt();
    assert!(prompt.skippable, "a reaction is optional: {prompt:?}");
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|o| o.target.clone())
            .collect::<Vec<_>>(),
        vec![Some(OptionTarget::CardInstance(instance(1)))],
        "the one option is the reaction, anchored to its card"
    );

    let session = session.pick(OptionTarget::CardInstance(instance(1)));
    assert_eq!(marks(&session), vec![1, 3], "the reaction resolved");
}

#[test]
fn two_simultaneous_forced_abilities_run_in_the_order_the_lead_picks() {
    let session = session_holding(&[FORCED_A, FORCED_B]).fire_at(TimingEvent::RoundEnded);

    assert!(
        marks(&session).is_empty(),
        "nothing resolves before the lead orders them"
    );
    let prompt = session.prompt();
    assert_eq!(prompt.kind, InputKind::PickSingle);
    assert!(
        !prompt.skippable,
        "forced abilities are mandatory: {prompt:?}"
    );
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|o| o.target.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(OptionTarget::CardInstance(instance(0))),
            Some(OptionTarget::CardInstance(instance(1))),
        ],
        "one option per forced ability, anchored to its card"
    );

    // The lead picks the second card's ability first; the other follows.
    let session = session
        .pick(OptionTarget::CardInstance(instance(1)))
        .pick(OptionTarget::CardInstance(instance(0)));
    assert_eq!(
        marks(&session),
        vec![2, 1],
        "they resolved in the lead's order"
    );
    assert_eq!(session.finish().outcome, EngineOutcome::Done);
}

#[test]
fn a_timing_point_fired_during_a_turn_comes_back_to_the_turn_menu() {
    let mut investigator = test_support::test_investigator(1);
    investigator
        .threat_area
        .push(CardInPlay::enter_play(CardCode::new(FORCED_A), instance(0)));
    let session = GameStateBuilder::new()
        .with_investigator(investigator)
        .open_turn(INV)
        .session()
        .fire_at(TimingEvent::RoundEnded);

    assert_eq!(marks(&session), vec![1], "the forced ability resolved");
    assert_eq!(
        session.prompt().target,
        Some(OptionTarget::TurnControl(INV)),
        "the turn the timing point interrupted is open again"
    );
}

/// The ending latches inside the coordinator's `at` cell, and the step's
/// `apply` boundary is what finalizes it — which a timing point fired without
/// the production scaffolding never reaches.
#[test]
fn a_forced_ability_that_reaches_a_resolution_point_ends_the_game_there() {
    let session = session_holding(&[RESOLVES]).fire_at(TimingEvent::RoundEnded);

    assert_event!(
        session.events(),
        Event::ScenarioResolved { ending }
            if *ending == ScenarioEnding::Resolution(ResolutionId::new(1))
    );
    assert_eq!(
        session.state().ending,
        Some(ScenarioEnding::Resolution(ResolutionId::new(1)))
    );
    // The one stack assertion here: #946's acceptance criteria name it, an
    // exception to standards.md § Test layering.
    assert!(
        session.state().continuations.is_empty(),
        "nothing is left to resolve"
    );
    assert_eq!(
        session.finish().outcome,
        EngineOutcome::Done,
        "the game is over"
    );
}

#[test]
fn lethal_damage_eliminates_the_last_investigator_and_ends_the_scenario() {
    let session = session_holding(&[]).take_damage(INV, 8);

    assert_eq!(
        session.state().investigators[&INV].status,
        game_core::state::Status::Defeated
    );
    assert_event!(
        session.events(),
        Event::ScenarioResolved {
            ending: ScenarioEnding::NoResolution
        }
    );
    assert_eq!(
        session.finish().outcome,
        EngineOutcome::Done,
        "the game is over"
    );
}

/// One investigator holding `held` in their threat area, `builder`'s staging,
/// and an act carrying `act`'s ability — the phase-end scans read the current
/// act — settled.
fn acting(act: &str, held: &[&str], builder: GameStateBuilder) -> TestSession {
    let mut state = builder
        .with_investigator(holding(held))
        .with_turn_order([INV])
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(act),
        clue_threshold: 0,
    }];
    TestSession::new(state)
}

/// The phase end is not a step: a session never rests with a phase anchor
/// exposed, so the builder stages the phase at its end and settling runs step
/// 3.4 — through the same production scaffolding a step uses. The phase-end
/// forced ability resolves before the transition to Upkeep.
#[test]
fn ending_the_enemy_phase_resolves_its_forced_abilities_then_starts_upkeep() {
    let session = acting(PHASE_END, &[], GameStateBuilder::new().ending_enemy_phase());

    assert_event_sequence!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Enemy
        },
        Event::ResourcesGained { amount: 4, .. },
        Event::PhaseStarted {
            phase: Phase::Upkeep
        },
    );
}

/// Settling an upkeep staged at its end runs steps 4.5 and 4.6 alone — the
/// draw-and-resource step does not re-run — so the phase end's forced ability
/// resolves, then the round end's, and the next round's Mythos phase begins.
#[test]
fn ending_upkeep_ends_the_phase_then_the_round_then_starts_mythos() {
    let session = acting(
        UPKEEP_END,
        &[FORCED_A],
        GameStateBuilder::new().ending_upkeep_phase(),
    );

    assert_eq!(
        marks(&session),
        vec![5, 1],
        "the phase-end forced ability, then the round-end one, and no upkeep resource"
    );
    assert_event_sequence!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Upkeep
        },
        Event::ResourcesGained { amount: 5, .. },
        Event::ResourcesGained { amount: 1, .. },
        Event::PhaseStarted {
            phase: Phase::Mythos
        },
    );
}
