//! The lone forced hit's firing path and its #466 acknowledge.
//!
//! Which forced abilities a timing event reaches is the one trigger scan's
//! answer ([`trigger_scan::collect_forced`]); two or more simultaneous hits
//! are ordered by the lead (`open_forced_resolution`, #213). What is left here
//! is the single hit: fire it through [`initiation::initiate`], the path the
//! ordered run and reactions share, and in interactive play surface it as a
//! one-option acknowledge before it resolves (#466).

use std::borrow::Cow;

use card_dsl::dsl::{Effect, EventTiming};

use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::dispatch::{initiation, reaction_windows, trigger_scan};
use crate::engine::outcome::{ChoiceOption, EngineOutcome, InputRequest, OptionId, ResumeToken};
use crate::engine::Cx;
use crate::state::{AcknowledgeForcedFrame, CardCode};

/// Queue the lone forced ability `event` reaches in the `bucket` cell: push its
/// effect frame (plus an
/// [`AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced) above it
/// in interactive mode) for the `drive` loop to resolve.
///
/// **Queues; does not resolve.** `Done` here means *the frame is on the stack*,
/// not *the effect has happened* — nothing is evaluated synchronously under the
/// frame model (#423). Callers with post-forced work resume via their own frame
/// and call this in tail position; see [`super::emit::queue_event`], the
/// chokepoint this backs, and
/// `docs/adr/0003-emitting-a-timing-point-queues-abilities.md` (#569).
///
/// At most one hit reaches here: 2+ simultaneous forced abilities route to the
/// lead-ordered run (`open_forced_resolution`, #213, Rules Reference p.17 — the
/// player orders simultaneous triggers, even in solo), so this path has no
/// ordering to choose. It fires through [`initiation::initiate`], the path the
/// ordered run and reactions share, so `event` supplies the same bindings
/// either way. Never returns `AwaitingInput`; an ability that no longer
/// resolves at its address returns `Rejected`.
#[must_use = "queue_forced_triggers only pushes the forced effect's frame; the \
              effect has not run when this returns (ADR 0003)"]
pub(crate) fn queue_forced_triggers(
    cx: &mut Cx,
    event: &TimingEvent,
    bucket: EventTiming,
) -> EngineOutcome {
    // Frame-driven forced run (Slice D, #423): `initiate` pushes the
    // candidate's effect root frame for the global `drive` loop to own; this
    // function does not drive. Callers under the loop (effect-eval emits) get the
    // forced effect driven next; callers with post-forced work (`end_turn`'s
    // rotation, the `GameEnd` resolution finalization) arm a resumption frame
    // before emitting and let the loop drive the forced frame then re-dispatch
    // the resumption.
    //
    // At most one hit reaches here: the coordinator / emit `<2` guard routes 2+
    // simultaneous forced abilities to the ordered forced-run frame
    // (`open_forced_resolution`, #213), so there is no ordering to preserve.
    let hits = trigger_scan::collect_forced(cx.state, event, bucket);
    debug_assert!(
        hits.len() <= 1,
        "queue_forced_triggers: expected 0/1 forced hit (2+ routes through \
         open_forced_resolution); got {}",
        hits.len(),
    );
    let Some(hit) = hits.first() else {
        return EngineOutcome::Done;
    };
    // The one firing path (#964): the lone hit resolves, binds from `event`
    // and records its use exactly as it would had a sibling triggered alongside
    // it and sent both to the lead's ordered run.
    let effect = match initiation::initiate(cx, hit, event) {
        Ok(effect) => effect,
        Err(refusal) => {
            return EngineOutcome::Rejected {
                reason: format!(
                    "queue_forced_triggers: {} at {:?} cannot be fired: {}",
                    hit.code,
                    hit.address,
                    Cow::from(refusal),
                )
                .into(),
            };
        }
    };
    // #466: in interactive play, surface the lone forced effect as a one-option
    // pick *before* it resolves. `initiate` already pushed the effect root frame;
    // push the ack *above* it so the `drive` loop hits the ack first (suspend),
    // and on resume pops it — then resolves the effect. queue_forced_triggers
    // still returns Done (push-frame contract), so emit callers stay correct.
    // Scoped to this single-hit path: in the 2+ ordered run the lead's ordering
    // pick is the only confirmation (no per-effect ack).
    //
    // …except when the effect *is* an advance (#558's slice 4, #562). See
    // `is_only_an_advance`.
    if cx.state.interactive_acknowledge && !is_only_an_advance(&effect) {
        cx.state.continuations.push(AcknowledgeForcedFrame {
            candidate: hit.clone(),
        });
    }
    EngineOutcome::Done
}

/// Whether a forced ability's effect does nothing but advance the act — in which
/// case the ability raises **no** #466 acknowledge of its own. This is #558's
/// deferred slice 4 (#562), and it is the fire-once rule that design already
/// stated: *"a forced ability whose effect is only an act/agenda advance
/// suppresses its `#466` confirm — the advance's `AwaitAck` is the single
/// flip-click"*.
///
/// **The advance already has a click, and it is the better one.** The advance's
/// own [`AwaitAck`](crate::state::AdvanceStep::AwaitAck) flip pick is anchored to
/// the act and reads "Advance"; a `#466` ack in front of it is a second prompt
/// saying the same thing, from the same card, under a prompt text that on 01110
/// is *identical* to the one its reverse raises a moment later. The objective
/// (*"If the Ghoul Priest is Defeated, advance."*) and the reverse are both
/// printed on What Have You Done?, so the player met *"Forced — What Have You
/// Done?"* twice with the flip in between and no way to tell which was which.
///
/// **Only a bare advance qualifies.** An ability that advances *and* does
/// something else keeps its acknowledge: the other half is a real effect the
/// player should confirm before it lands, and #466's rule is about effects, not
/// about advances. No corpus card has that shape today; the predicate is written
/// so that one arriving inherits the acknowledge rather than losing it silently.
///
/// The agenda has no counterpart to suppress — an agenda advances from the doom
/// threshold, which is engine machinery rather than a forced ability, so its flip
/// pick was never stacked on top of one.
fn is_only_an_advance(effect: &Effect) -> bool {
    matches!(effect, Effect::AdvanceCurrentAct)
}

/// Display name for the card a forced ability is printed on, for the
/// [`AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced) prompt.
/// Resolved via the registry; falls back to the raw code when no
/// registry/metadata is available (tests).
fn forced_source_name(code: &CardCode) -> String {
    card_registry::current()
        .and_then(|r| (r.metadata_for)(code))
        .map_or_else(|| code.0.clone(), |m| m.name.clone())
}

/// Drive a [`Continuation::AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced)
/// frame (#466): suspend with a one-option `PickSingle` naming the source. The
/// pick precedes the forced effect's resolution ("confirm before the effect"),
/// and for an act/agenda reverse it is the whole of what an advance asks (#858).
pub(crate) fn drive_acknowledge_forced(cx: &mut Cx) -> EngineOutcome {
    let candidate = &cx
        .state
        .continuations
        .top_expect::<AcknowledgeForcedFrame>()
        .candidate;
    let name = forced_source_name(&candidate.code);
    let anchor = reaction_windows::candidate_anchor(candidate);
    EngineOutcome::AwaitingInput {
        request: InputRequest::pick_single(
            format!("Forced — {name}"),
            vec![ChoiceOption::new(OptionId(0), "Resolve").at(anchor)],
        ),
        resume_token: ResumeToken(0),
    }
}

/// Resume an [`AcknowledgeForced`](crate::state::Continuation::AcknowledgeForced)
/// frame: validate the single option, pop the frame, and return `Done` so the
/// `drive` loop resolves the forced effect beneath.
pub(crate) fn resume_acknowledge_forced(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    if !matches!(response, InputResponse::PickSingle(OptionId(0))) {
        return EngineOutcome::Rejected {
            reason: "resume_acknowledge_forced: expected the single forced-resolution option"
                .into(),
        };
    }
    cx.state
        .continuations
        .pop_expect::<AcknowledgeForcedFrame>();
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::outcome::OptionTarget;
    use crate::state::{
        AbilityAddress, AbilitySource, Agenda, CandidateSource, CardInstanceId, Continuation,
        GameStateBuilder, InvestigatorId, LocationId, ResolutionCandidate,
    };

    #[test]
    fn acknowledge_forced_suspends_then_pops_on_pick() {
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(AcknowledgeForcedFrame {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Location(LocationId(1))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        // Drive: one-option suspend.
        let out = drive_acknowledge_forced(&mut cx);
        match out {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }

        // Resume with the single option: frame pops, returns Done.
        let out = resume_acknowledge_forced(&mut cx, &InputResponse::PickSingle(OptionId(0)));
        assert!(matches!(out, EngineOutcome::Done));
        assert!(
            cx.state.continuations.is_empty(),
            "the AcknowledgeForced frame must be popped on resume"
        );
    }

    #[test]
    fn acknowledge_forced_rejects_non_pick_response() {
        // Validate-first: a Confirm/Skip (not the single PickSingle) is rejected
        // and leaves the frame in place.
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(AcknowledgeForcedFrame {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Location(LocationId(1))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = resume_acknowledge_forced(&mut cx, &InputResponse::Confirm);
        assert!(matches!(out, EngineOutcome::Rejected { .. }));
        assert!(
            matches!(
                cx.state.continuations.top(),
                Some(Continuation::AcknowledgeForced(_))
            ),
            "a rejected resume must leave the frame in place"
        );
    }

    #[test]
    fn acknowledge_forced_anchors_the_option_to_its_source_card() {
        // A forced ability on an in-play instance surfaces a one-option pick
        // anchored to that card (#553), not Global.
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(AcknowledgeForcedFrame {
            candidate: ResolutionCandidate::new(
                CardCode::new("01020"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::InPlay(CardInstanceId(5))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
                assert_eq!(
                    request.options[0].target,
                    Some(OptionTarget::CardInstance(CardInstanceId(5))),
                );
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }

    #[test]
    fn acknowledge_forced_anchors_a_location_source_to_its_map_node() {
        // A location's own forced ability (the Attic's on-enter horror) surfaces a
        // one-option pick anchored to the location on the map (#553), not Global.
        let mut state = GameStateBuilder::default().build();
        state.continuations.push(AcknowledgeForcedFrame {
            candidate: ResolutionCandidate::new(
                CardCode::new("01113"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Location(LocationId(3))),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
                assert_eq!(
                    request.options[0].target,
                    Some(OptionTarget::Location(LocationId(3))),
                );
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }

    #[test]
    fn acknowledge_forced_anchors_an_agenda_source_to_the_agenda_card() {
        // A forced ability on the current agenda (What's Going On?! 01105's
        // on-advance reverse) anchors its "Resolve" to the agenda card (#556).
        let mut state = GameStateBuilder::default().build();
        state.agenda_deck = vec![Agenda {
            code: CardCode::new("01105"),
            doom_threshold: 3,
        }];
        state.agenda_index = 0;
        state.continuations.push(AcknowledgeForcedFrame {
            candidate: ResolutionCandidate::new(
                CardCode::new("01105"),
                InvestigatorId(1),
                AbilityAddress::Printed(0),
                CandidateSource::Ability(AbilitySource::Agenda),
            ),
        });
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        match drive_acknowledge_forced(&mut cx) {
            EngineOutcome::AwaitingInput { request, .. } => {
                assert_eq!(request.options.len(), 1, "forced ack is a one-option pick");
                assert_eq!(request.options[0].target, Some(OptionTarget::Agenda));
            }
            other => panic!("expected one-option suspend, got {other:?}"),
        }
    }
}
