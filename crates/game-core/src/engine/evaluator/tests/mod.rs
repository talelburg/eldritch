use card_dsl::card_data::CardMetadata;
use card_dsl::dsl::{self, Choose, TestOutcome};

use super::*;
use crate::action::InputResponse;
use crate::engine::dispatch::coordinator;
use crate::state::{
    Act, Agenda, CardInPlay, DifficultyBasis, FastActorScope, FastWindowKind, GameStateBuilder,
    InFlightSkillTest, PhaseStep, RecordedModifierKind, SkillKind, SkillTestFollowUp, SkillTestId,
    SkillTestStep, Status,
};
use crate::{assert_event, assert_no_event, test_support};

mod act_progress;
mod action_restrictions;
mod can_change_state;
mod choose_one;
mod chosen_targets;
mod conditions;
mod control_flow;
mod deal;
mod discard_self;
mod discover_clue;
mod eval_context;
mod gain_resources;
mod leaf_effects;
mod modify;
mod place_doom;
mod search_deck;

fn ctx(id: u32) -> EvalContext {
    EvalContext::for_controller(InvestigatorId(id))
}

/// A state with investigator 1 standing on location 10, which holds `clues`.
fn state_with_clues_at_location(clues: u8) -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LocationId(10));
    let mut loc = test_support::test_location(10, "Study");
    loc.clues = clues;
    GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc)
        .build()
}

/// Bounded effect driver — the deleted production `drive_effect_to_base`,
/// now test-only (Slice D #423). Steps the top contiguous run until it
/// shrinks to `base` (run complete → `Done`) or a leaf suspends for a pick
/// (`AwaitingInput`), WITHOUT touching fixture frames beneath `base` (an
/// in-flight `SkillTest` carrying `tested_location`, say). The production
/// path no longer needs this — the global `drive` loop drives the parked run
/// and then *does* advance the enclosing frame — but a unit test parks
/// fixtures it does not want driven, so it drives bounded instead.
///
/// The run includes the timing coordinator, because a leaf can emit a
/// triggering condition whose *own resolution* the coordinator performs —
/// `Effect::DiscoverClue` since #703 only caps the count and emits, and the
/// clues move at the coordinator's resolve step. Stopping at the `EmitEvent`
/// frame would leave the effect half-resolved in a way `apply` never does.
fn drive_effect_run_to(cx: &mut Cx, base: usize) -> EngineOutcome {
    loop {
        if cx.state.continuations.len() <= base {
            return EngineOutcome::Done;
        }
        let outcome = match cx.state.continuations.last() {
            Some(Continuation::Effect(_)) => step_effect_frame(cx),
            Some(Continuation::EmitEvent { .. }) => coordinator::dispatch_emit_event(cx),
            Some(Continuation::TimingPoint { .. }) => coordinator::dispatch_timing_point(cx),
            // `Effect::Deal` parks one of these and returns in tail position
            // (#727): the two steps of dealing the damage are the frame's,
            // not the effect walk's. The real `drive` loop dispatches it, so
            // this bounded stand-in must too, or `Deal` in a unit test
            // assigns damage that is never placed.
            Some(Continuation::DealDamage { .. }) => combat::drive_deal_damage(cx),
            _ => return EngineOutcome::Done,
        };
        match outcome {
            EngineOutcome::Done => {}
            other => return other,
        }
    }
}

/// Push an effect's root frame and drive **only that run** to completion or
/// a controller-pick suspension — the test-only successor to the deleted
/// `apply_effect` bounded entry (Slice D #423). `Done` stays `Done`; a 2+
/// controller pick stays `AwaitingInput`.
fn run(cx: &mut Cx, effect: &Effect, ctx: EvalContext) -> EngineOutcome {
    let base = cx.state.continuations.len();
    push_effect(cx, effect, ctx);
    drive_effect_run_to(cx, base)
}

/// Resume a suspended-in-place effect choice with `PickSingle(i)` — the same
/// path `apply(ResolveInput)` routes to (#422). Records the pick on the top
/// `Leaf` via `resume_effect_choice` (which now just cedes to the global
/// loop), then drives the resumed top effect run **bounded** — in a unit
/// test there is no `apply()`→`drive()` afterward to step it (Slice D #423).
fn resume_pick(state: &mut GameState, events: &mut Vec<Event>, i: u32) -> EngineOutcome {
    let mut cx = Cx { state, events };
    let recorded = choice::resume_effect_choice(&mut cx, &InputResponse::PickSingle(OptionId(i)));
    // A reject (bad pick / top not a Leaf) propagates as-is; otherwise the
    // pick is recorded and the resumed run is driven bounded (base = depth
    // just below the top contiguous Effect run, so fixtures stay untouched).
    if !matches!(recorded, EngineOutcome::Done) {
        return recorded;
    }
    let base = cx
        .state
        .continuations
        .iter()
        .rposition(|c| !matches!(c, Continuation::Effect(_)))
        .map_or(0, |idx| idx + 1);
    drive_effect_run_to(&mut cx, base)
}

/// Number of options offered by a suspending `AwaitingInput` (replaces the
/// former `ChoiceFrame.offered.len()` assertion — #422).
fn offered_count(outcome: &EngineOutcome) -> usize {
    match outcome {
        EngineOutcome::AwaitingInput { request, .. } => request.options.len(),
        other => panic!("expected AwaitingInput, got {other:?}"),
    }
}

/// Assert the top frame is an effect node suspended in place for a pick.
#[track_caller]
fn assert_suspended_leaf(state: &GameState) {
    assert!(
        matches!(
            state.continuations.last(),
            Some(Continuation::Effect(EffectFrame::Leaf { .. })),
        ),
        "expected a suspended effect Leaf frame on top, got {:?}",
        state.continuations.last(),
    );
}

fn state_with_cards_in_play(codes: &[&str]) -> (GameState, InvestigatorId) {
    let id = InvestigatorId(1);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play = codes
        .iter()
        .enumerate()
        .map(|(i, c)| {
            CardInPlay::enter_play(
                CardCode::new(*c),
                #[allow(clippy::cast_possible_truncation)]
                CardInstanceId(i as u32),
            )
        })
        .collect();
    let state = GameStateBuilder::new().with_investigator(inv).build();
    (state, id)
}
