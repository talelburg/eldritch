//! The prompt table: what a prompt frame asks and who answers it (ADR 0019),
//! read from the frame.
//!
//! Every [`InputRequest`] the engine surfaces is built here, from the frame on
//! top of the continuation stack, by [`request`] and the per-kind helpers it
//! calls. Nothing else assembles one: a native's own pick is stored on the
//! effect frame `NativeCx::suspend` re-pushes, and suspends through [`suspend`]
//! like any step. A step that suspends leaves its frame at
//! rest on top and returns [`suspend`], and a frame pushed by a step that then
//! returns `Done` is surfaced the same way when the `drive` loop meets it. So
//! every prompt can be shown again from the state alone: a state settled with
//! any prompt frame on top shows its prompt, and a native whose exit pushes a
//! framework prompt (a spawn's engagement tie) just returns, the prompt
//! surfacing after it.
//!
//! The table decides only *what* is asked. *Whether* a frame rests is its drive
//! arm's call, because some frames do work before they rest: a window sweeps
//! its lapsed candidates, a deal of damage assigns its uncontested points, a
//! hand-discard cost pays a lone candidate. Those arms do their writes, then
//! return [`suspend`].

use crate::card_registry::CardRegistry;
use crate::engine::dispatch::actions::take;
use crate::engine::dispatch::{
    advance_reverse, cards, choice, combat, cursor, encounter, forced_triggers, hand_discard_cost,
    hunters, phases, reaction_windows, skill_test, slots,
};
use crate::engine::enumerate;
use crate::engine::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, Responder, ResumeToken,
};
use crate::state::{
    AttackLoopFrame, AttackLoopStage, Continuation, DealDamageFrame, DealDamageStep, EffectFrame,
    FastWindowFrame, GameState, InvestigatorId, InvestigatorTurnFrame, SkillTestStep,
    TimingPointWindowFrame,
};

/// What `frame` asks when it rests on top of `state`, answered by whom ADR
/// 0019's responder table names, or `None` when it asks nothing: a Driven or
/// Inert frame, or a Prompt-profile frame at a step that is not a question (a
/// skill test between its commit window and its result pause, an effect node
/// not yet stepped).
///
/// This is where a request gets its [`Responder`]: the table pairs each
/// question with who answers it, and this attaches the one to the other.
///
/// Reads `state` as well as the frame where the question depends on the board:
/// the turn menu's legal actions, a window's resolution options, the skill-test
/// under a substitution prompt, who the lead is now.
pub(crate) fn request(
    frame: &Continuation,
    state: &GameState,
    cards: &CardRegistry,
) -> Option<InputRequest> {
    table(frame, state, cards).map(|(request, responder)| request.answered_by(responder))
}

/// The prompt table: what `frame` asks and who answers it (ADR 0019), or
/// `None` when it asks nothing.
///
/// One exhaustive arm per [`Continuation`] kind with no wildcard, so a new
/// frame kind does not compile until it says what it asks and who answers. It
/// sits beside the frame's [profile](Continuation::profile), which says whether
/// the frame may rest at all; this says what it asks when it does.
#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive match arm per `Continuation` kind"
)]
fn table(
    frame: &Continuation,
    state: &GameState,
    cards: &CardRegistry,
) -> Option<(InputRequest, Responder)> {
    match frame {
        Continuation::InvestigatorTurn(InvestigatorTurnFrame {
            investigator,
            ending,
        }) => (!ending).then(|| {
            (
                turn_menu(state, cards, *investigator),
                Responder::Investigator(*investigator),
            )
        }),
        // An empty event window or forced run is closed on sight, never shown.
        // A forced run's order is the lead's (`Priority_of_Simultaneous_Resolution.md`:
        // *"the lead investigator determines the order in which the abilities
        // resolve"*); a reaction window's options each belong to the
        // investigator they are offered to.
        Continuation::TimingPointWindow(TimingPointWindowFrame { candidates, .. }) => {
            (!candidates.is_empty()).then(|| {
                let responder = if frame.is_forced() {
                    lead(state)
                } else {
                    Responder::Several
                };
                (
                    reaction_windows::resolution_request(state, cards, frame, candidates),
                    responder,
                )
            })
        }
        // A framework Fast window holding reaction candidates resolves them like
        // any window; without, it offers the eligible fast plays. Either way
        // each option belongs to the investigator it is offered to.
        Continuation::FastWindow(FastWindowFrame { candidates, .. }) => Some((
            if candidates.is_empty() {
                reaction_windows::fast_window_request(state, cards)
            } else {
                reaction_windows::resolution_request(state, cards, frame, candidates)
            },
            Responder::Several,
        )),
        Continuation::AdvanceReverse(advance) => {
            advance_reverse::ack_request(advance).map(|request| (request, lead(state)))
        }
        Continuation::AcknowledgeForced(acknowledge) => Some((
            forced_triggers::acknowledge_request(cards, acknowledge),
            Responder::Investigator(acknowledge.candidate.controller),
        )),
        Continuation::SkillTest(test) => match test.continuation {
            SkillTestStep::AwaitingCommit => Some(skill_test::commit_request(state, cards)),
            SkillTestStep::AcknowledgeOutcome if state.interactive_acknowledge => {
                Some(skill_test::acknowledge_request())
            }
            SkillTestStep::AcknowledgeOutcome
            | SkillTestStep::PreCommitWindow
            | SkillTestStep::PreTokenWindow
            | SkillTestStep::Resolving
            | SkillTestStep::DetermineOutcome
            | SkillTestStep::FireOnCommit
            | SkillTestStep::ApplyFollowUp
            | SkillTestStep::ApplyResultEffect
            | SkillTestStep::ApplyTokenResult
            | SkillTestStep::FireOnResolution { .. }
            | SkillTestStep::PostRetaliate
            | SkillTestStep::PostOnResolution => None,
        }
        .map(|request| (request, Responder::Investigator(test.investigator))),
        Continuation::SubstitutionPrompt(substitution) => Some((
            skill_test::substitution_request(state, substitution),
            Responder::Investigator(substitution.investigator),
        )),
        Continuation::HunterMove(choice) => {
            Some((hunters::choice_request(state, cards, choice), lead(state)))
        }
        Continuation::SpawnEngage(pending) => Some((
            encounter::spawn_engage_request(state, cards, pending),
            lead(state),
        )),
        Continuation::HandSizeDiscard(queue) => queue.remaining.first().map(|&head| {
            (
                phases::hand_size_discard_request(state, cards, head),
                Responder::Investigator(head),
            )
        }),
        Continuation::Mulligan(queue) => queue.remaining.first().map(|&head| {
            (
                cards::mulligan_request(state, cards, head),
                Responder::Investigator(head),
            )
        }),
        Continuation::EncounterDraw(queue) => {
            head(&queue.remaining).map(|head| (encounter::encounter_draw_request(state), head))
        }
        Continuation::SlotDiscard(slot) => Some((
            slots::slot_discard_request(state, cards, slot),
            Responder::Investigator(slot.investigator),
        )),
        Continuation::AdditionalActionChoice(pending) => Some((
            take::additional_action_request(state, cards, pending),
            Responder::Investigator(pending.investigator),
        )),
        Continuation::HandDiscardCost(cost) => Some((
            hand_discard_cost::request(state, cards, cost),
            Responder::Investigator(cost.investigator),
        )),
        Continuation::AttackLoop(AttackLoopFrame {
            investigator,
            remaining_attackers,
            stage,
            ..
        }) => match stage {
            AttackLoopStage::PickOrder => Some((
                combat::order_pick_request(state, cards, *investigator, remaining_attackers),
                Responder::Investigator(*investigator),
            )),
            AttackLoopStage::Attacking => None,
        },
        Continuation::DealDamage(deal @ DealDamageFrame { step, .. }) => match step {
            DealDamageStep::Distribute { .. } => Some((
                combat::distribution_request(state, cards, deal),
                Responder::Investigator(deal.investigator),
            )),
            DealDamageStep::Announce | DealDamageStep::Finish => None,
        },
        // An effect's choice is its controller's.
        Continuation::Effect(
            EffectFrame::Leaf { prompt, ctx, .. } | EffectFrame::Designated { prompt, ctx, .. },
        ) => prompt.as_ref().map(|prompt| {
            (
                choice::effect_request(prompt),
                Responder::Investigator(ctx.controller),
            )
        }),
        Continuation::Effect(EffectFrame::Seq { .. } | EffectFrame::ForEach { .. })
        | Continuation::EmitEvent(_)
        | Continuation::TimingPoint(_)
        | Continuation::PlayerDraw(_)
        | Continuation::EncounterCard(_)
        | Continuation::PlayFromHand(_)
        | Continuation::MoveEnter(_)
        | Continuation::ActionResolution(_)
        | Continuation::Elimination(_)
        | Continuation::ScenarioEnd(_)
        | Continuation::MythosPhase(_)
        | Continuation::InvestigationPhase(_)
        | Continuation::EnemyPhase(_)
        | Continuation::UpkeepPhase(_) => None,
    }
}

/// The lead now, answering a group decision.
///
/// That is [`cursor::the_lead`] while anyone is Active. Some group decisions
/// still run after the scenario ends: an advance's acknowledge and a forced
/// run are part of the ending's resolution, and can surface once every
/// investigator is eliminated. Nobody is Active then, and the stored
/// [`GameState::lead`] answers.
///
/// # Panics
///
/// If nobody is Active and the state stores no lead, which only a state with
/// no seats does. A group decision then has nobody to make it.
fn lead(state: &GameState) -> Responder {
    Responder::Lead(
        cursor::the_lead(state)
            .or(state.lead)
            .expect("a group decision is asked in a state with no seats"),
    )
}

/// A queue frame's head: the investigator it is asking now. `None` for an
/// empty queue, which asks nothing.
fn head(remaining: &[InvestigatorId]) -> Option<Responder> {
    remaining.first().copied().map(Responder::Investigator)
}

/// Suspend on the frame on top of `state`: the `AwaitingInput` carrying what
/// it asks ([`request`]). The one way the engine suspends — a step that rests
/// its frame on top returns this.
///
/// # Panics
///
/// If the stack is empty or its top frame asks nothing. Either is an engine
/// bug: the caller suspended on a frame that is not at rest as a prompt.
pub(crate) fn suspend(state: &GameState, cards: &CardRegistry) -> EngineOutcome {
    let top = state
        .continuations
        .top()
        .expect("prompt::suspend: the continuation stack is empty");
    let request = request(top, state, cards)
        .unwrap_or_else(|| panic!("prompt::suspend: the top frame asks nothing: {top:?}"));
    EngineOutcome::AwaitingInput {
        request,
        // TODO(#458): deterministic resume tokens; 0 is the placeholder every
        // `AwaitingInput` carries.
        resume_token: ResumeToken(0),
    }
}

/// The open-turn action menu: the legal-action enumeration as a `PickSingle`.
/// `OptionId(i)` indexes
/// [`legal_actions`](crate::engine::enumerate::legal_actions); the
/// `InvestigatorTurn` arm of `resolve_input` re-enumerates and dispatches.
///
/// The prompt itself is anchored to the acting investigator's turn control, so a
/// host can suppress its "Choose an action" text structurally rather than by
/// matching the string (ADR 0011).
fn turn_menu(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
) -> InputRequest {
    let options = enumerate::legal_actions(state, cards)
        .iter()
        .enumerate()
        .map(|(i, a)| {
            ChoiceOption::new(
                OptionId(u32::try_from(i).unwrap_or(u32::MAX)),
                a.label(state, cards),
            )
            .maybe_at(a.target(state))
        })
        .collect();
    InputRequest::pick_single("Choose an action", options)
        .at(OptionTarget::TurnControl(investigator))
}
