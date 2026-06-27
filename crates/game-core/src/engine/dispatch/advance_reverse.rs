//! Resumable act/agenda advance (#482): the `AdvanceReverse` continuation frame
//! and its driver. See the `Continuation::AdvanceReverse` doc.

use crate::action::InputResponse;
use crate::event::Event;
use crate::state::{AdvanceDeck, AdvanceStep, Continuation};

use super::super::outcome::{EngineOutcome, InputRequest, ResumeToken};
use super::Cx;

/// Read the top `AdvanceReverse` frame's fields. The frame is the top
/// continuation whenever the driver / resume runs (the `drive` loop /
/// `resolve_input` route here only with it on top).
fn top(cx: &Cx) -> (AdvanceDeck, usize, crate::state::CardCode, AdvanceStep) {
    match cx.state.continuations.last() {
        Some(Continuation::AdvanceReverse {
            deck,
            from,
            leaving_code,
            step,
        }) => (*deck, *from, leaving_code.clone(), *step),
        other => {
            unreachable!("advance_reverse: AdvanceReverse frame must be on top, got {other:?}")
        }
    }
}

/// Set the top `AdvanceReverse` frame's step cursor.
fn set_step(cx: &mut Cx, next: AdvanceStep) {
    match cx.state.continuations.last_mut() {
        Some(Continuation::AdvanceReverse { step, .. }) => *step = next,
        other => {
            unreachable!("advance_reverse: AdvanceReverse frame must be on top, got {other:?}")
        }
    }
}

fn advanced_event(deck: AdvanceDeck, from: usize) -> Event {
    match deck {
        AdvanceDeck::Act => Event::ActAdvanced { from },
        AdvanceDeck::Agenda => Event::AgendaAdvanced { from },
    }
}

fn reverse_timing(deck: AdvanceDeck, code: crate::state::CardCode) -> super::emit::TimingEvent {
    match deck {
        AdvanceDeck::Act => super::emit::TimingEvent::ActAdvanced { code },
        AdvanceDeck::Agenda => super::emit::TimingEvent::AgendaAdvanced { code },
    }
}

/// Human label for the acknowledge prompt (1-based, e.g. "Agenda 1 advanced").
fn ack_prompt(deck: AdvanceDeck, from: usize) -> String {
    let what = match deck {
        AdvanceDeck::Act => "Act",
        AdvanceDeck::Agenda => "Agenda",
    };
    format!("{what} {} advanced — acknowledge.", from + 1)
}

/// Drive the top `AdvanceReverse` frame one step (#482). `AwaitAck` pushes the
/// observable `…Advanced` event and, when `interactive_acknowledge` is set,
/// suspends with a `Confirm` (the cursor stays at `AwaitAck` until `resume`).
/// `FireReverse` fires the leaving card's Forced reverse via `emit_event`
/// (queued; may suspend). `Finalize` bumps the deck cursor and pops the frame.
pub(super) fn drive(cx: &mut Cx) -> EngineOutcome {
    let (deck, from, leaving_code, step) = top(cx);
    match step {
        AdvanceStep::AwaitAck => {
            cx.events.push(advanced_event(deck, from));
            if cx.state.interactive_acknowledge {
                // Suspend for the acknowledge; cursor stays at AwaitAck. `resume`
                // advances to FireReverse on Confirm.
                return EngineOutcome::AwaitingInput {
                    request: InputRequest::confirm(ack_prompt(deck, from)),
                    resume_token: ResumeToken(0),
                };
            }
            set_step(cx, AdvanceStep::FireReverse);
            EngineOutcome::Done
        }
        AdvanceStep::FireReverse => {
            // Pre-advance BEFORE emitting so a suspending reverse resumes at
            // Finalize once its frames pop.
            set_step(cx, AdvanceStep::Finalize);
            super::emit::emit_event(cx, &reverse_timing(deck, leaving_code))
        }
        AdvanceStep::Finalize => {
            finalize(cx, deck, from);
            EngineOutcome::Done
        }
    }
}

/// Bump the deck cursor (RR order: after the reverse resolved) and pop the frame.
fn finalize(cx: &mut Cx, deck: AdvanceDeck, from: usize) {
    match deck {
        AdvanceDeck::Agenda => {
            cx.state.agenda_doom = 0;
            cx.state.agenda_index += 1;
            assert!(
                cx.state.agenda_index < cx.state.agenda_deck.len(),
                "advance_reverse: agenda {from} advanced past the end without a resolution \
                 (terminal agendas carry a resolution point); malformed scenario data",
            );
        }
        AdvanceDeck::Act => {
            cx.state.act_index += 1;
            assert!(
                cx.state.act_index < cx.state.act_deck.len(),
                "advance_reverse: act {from} advanced past the end without a resolution \
                 (terminal acts carry a resolution point); malformed scenario data",
            );
        }
    }
    let popped = cx.state.continuations.pop();
    debug_assert!(
        matches!(popped, Some(Continuation::AdvanceReverse { .. })),
        "advance_reverse: Finalize must pop the AdvanceReverse frame, popped {popped:?}",
    );
}

/// Resume the acknowledge pause (#482): a `Confirm` at `AwaitAck` advances the
/// cursor to `FireReverse`; the `drive` loop then fires the reverse. Validate-
/// first: a non-`Confirm`, or a frame past `AwaitAck`, rejects untouched.
pub(super) fn resume(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let (_, _, _, step) = top(cx);
    if !matches!(step, AdvanceStep::AwaitAck) {
        return EngineOutcome::Rejected {
            reason: format!("advance acknowledge: not at the acknowledge step (step {step:?})")
                .into(),
        };
    }
    if !matches!(response, InputResponse::Confirm) {
        return EngineOutcome::Rejected {
            reason: format!(
                "advance acknowledge: expected InputResponse::Confirm, got {response:?}"
            )
            .into(),
        };
    }
    set_step(cx, AdvanceStep::FireReverse);
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AdvanceDeck, AdvanceStep, Agenda, CardCode, Continuation};
    use crate::test_support::GameStateBuilder;

    fn state_advancing_agenda(interactive: bool) -> crate::state::GameState {
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![
            Agenda {
                code: CardCode("_a1".into()),
                doom_threshold: 1,
                resolution: None,
            },
            Agenda {
                code: CardCode("_a2".into()),
                doom_threshold: 3,
                resolution: None,
            },
        ];
        state.agenda_index = 0;
        state.interactive_acknowledge = interactive;
        // An AdvanceReverse frame as advance_agenda would push it (leaving = a1).
        state.continuations.push(Continuation::AdvanceReverse {
            deck: AdvanceDeck::Agenda,
            from: 0,
            leaving_code: CardCode("_a1".into()),
            step: AdvanceStep::AwaitAck,
        });
        state
    }

    /// Flag off: the frame drives straight through (no registry ⇒ no reverse) and
    /// the agenda cursor bumps at Finalize, the frame popping itself.
    #[test]
    fn advance_reverse_drives_through_when_not_interactive() {
        use crate::event::Event;
        let mut state = state_advancing_agenda(false);
        let mut events = Vec::new();
        let out = crate::engine::dispatch::drive(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            EngineOutcome::Done,
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.agenda_index, 1, "cursor bumped at Finalize");
        assert!(
            !state
                .continuations
                .iter()
                .any(|c| matches!(c, Continuation::AdvanceReverse { .. })),
            "frame popped"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::AgendaAdvanced { from: 0 })));
    }

    /// Flag on: the frame suspends at the acknowledge Confirm before firing the
    /// reverse — the cursor has NOT bumped yet.
    #[test]
    fn advance_reverse_pauses_for_acknowledge_when_interactive() {
        use crate::InputKind;
        let mut state = state_advancing_agenda(true);
        let mut events = Vec::new();
        let out = crate::engine::dispatch::drive(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            EngineOutcome::Done,
        );
        let EngineOutcome::AwaitingInput { request, .. } = &out else {
            panic!("expected the acknowledge Confirm, got {out:?}");
        };
        assert_eq!(request.kind, InputKind::Confirm);
        assert_eq!(
            state.agenda_index, 0,
            "cursor must NOT bump before the reverse resolves"
        );
    }
}
