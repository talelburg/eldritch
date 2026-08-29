//! Resumable act/agenda advance (#482): the `AdvanceReverse` continuation frame
//! and its driver. See the `Continuation::AdvanceReverse` doc.

use crate::action::InputResponse;
use crate::event::Event;
use crate::state::{AdvanceDeck, AdvanceStep, AdvanceTrigger, Continuation};

use super::super::outcome::{
    ChoiceOption, EngineOutcome, InputRequest, OptionId, OptionTarget, ResumeToken,
};
use super::Cx;

/// Read the top `AdvanceReverse` frame's fields. The frame is the top
/// continuation whenever the driver / resume runs (the `drive` loop /
/// `resolve_input` route here only with it on top).
fn top(
    cx: &Cx,
) -> (
    AdvanceDeck,
    usize,
    crate::state::CardCode,
    AdvanceStep,
    AdvanceTrigger,
) {
    match cx.state.continuations.last() {
        Some(Continuation::AdvanceReverse {
            deck,
            from,
            leaving_code,
            step,
            trigger,
        }) => (*deck, *from, leaving_code.clone(), *step, *trigger),
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
/// observable `…Advanced` event and, for a **forced** advance in interactive
/// mode, suspends with a one-option on-card `PickSingle` anchored to the
/// act/agenda (the flip pick — cursor stays at `AwaitAck` until `resume`); a
/// **deliberate** advance skips the ack and falls through (#558). `FireReverse`
/// fires the leaving card's Forced reverse via `queue_event` (queued; may
/// suspend). `Finalize` bumps the deck cursor and pops the frame.
pub(super) fn drive(cx: &mut Cx) -> EngineOutcome {
    let (deck, from, leaving_code, step, trigger) = top(cx);
    match step {
        AdvanceStep::AwaitAck => {
            cx.events.push(advanced_event(deck, from));
            // Fire-once, on-card (#558): a FORCED advance surfaces the flip as a
            // one-option pick anchored to the act/agenda; a DELIBERATE advance was
            // already the player's choice (the `AdvanceAct` action / round-end
            // objective), so skip the ack and fall through to fire the reverse.
            if cx.state.interactive_acknowledge && matches!(trigger, AdvanceTrigger::Forced) {
                let anchor = match deck {
                    AdvanceDeck::Act => OptionTarget::Act,
                    AdvanceDeck::Agenda => OptionTarget::Agenda,
                };
                return EngineOutcome::AwaitingInput {
                    request: InputRequest::pick_single(
                        ack_prompt(deck, from),
                        vec![ChoiceOption::new(OptionId(0), "Advance").at(anchor)],
                    ),
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
            super::emit::queue_event(cx, &reverse_timing(deck, leaving_code))
        }
        AdvanceStep::Finalize => {
            finalize(cx, deck, from);
            EngineOutcome::Done
        }
    }
}

/// Bump the deck cursor (RR order: after the reverse resolved) and pop the frame.
///
/// **A terminal card does not bump the cursor.** Terminality is structural — the
/// advancing card is the last in its deck — rather than a flag the card carries
/// (ADR 0013), so there is no next card to become current and the cursor stays
/// where it is. What must have happened instead is that the reverse *ended the
/// scenario*: either by running
/// [`Effect::ReachResolution`](crate::dsl::Effect::ReachResolution), or by
/// draining the last active investigator into `check_all_defeated`. Asserting
/// that here is the inverse of the past-the-end guard this replaced, and it fails
/// at the card whose reverse forgot to reach an ending rather than at a cursor
/// that has run off the end of the deck.
fn finalize(cx: &mut Cx, deck: AdvanceDeck, from: usize) {
    // Doom leaves the agenda whether or not a next one becomes current: RR step
    // 1 removes the tokens, and only step 3 has a next card to move to.
    if matches!(deck, AdvanceDeck::Agenda) {
        cx.state.agenda_doom = 0;
    }
    let (cursor, deck_len, label) = match deck {
        AdvanceDeck::Agenda => (
            &mut cx.state.agenda_index,
            cx.state.agenda_deck.len(),
            "agenda",
        ),
        AdvanceDeck::Act => (&mut cx.state.act_index, cx.state.act_deck.len(), "act"),
    };
    let terminal = from + 1 >= deck_len;
    if !terminal {
        *cursor += 1;
    }
    assert!(
        !terminal || cx.state.ending.is_some(),
        "advance_reverse: terminal {label} {from} finished without an ending latched — its \
         reverse reached no resolution point and defeated nobody; malformed scenario data",
    );
    let popped = cx.state.continuations.pop();
    debug_assert!(
        matches!(popped, Some(Continuation::AdvanceReverse { .. })),
        "advance_reverse: Finalize must pop the AdvanceReverse frame, popped {popped:?}",
    );
}

/// Resume the acknowledge pause (#558): the single "Advance" pick at `AwaitAck`
/// advances the cursor to `FireReverse`; the `drive` loop then fires the reverse.
/// `resume` only runs when `drive` paused here — which happens only for a forced,
/// interactive advance — so `PickSingle(OptionId(0))` is the sole valid response.
/// Validate-first: any other response, or a frame past `AwaitAck`, rejects
/// untouched.
pub(super) fn resume(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let (_, _, _, step, _) = top(cx);
    if !matches!(step, AdvanceStep::AwaitAck) {
        return EngineOutcome::Rejected {
            reason: format!("advance acknowledge: not at the acknowledge step (step {step:?})")
                .into(),
        };
    }
    if !matches!(response, InputResponse::PickSingle(OptionId(0))) {
        return EngineOutcome::Rejected {
            reason: format!(
                "advance acknowledge: expected the single advance pick, got {response:?}"
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
    use crate::state::{
        Act, AdvanceDeck, AdvanceStep, AdvanceTrigger, Agenda, CardCode, Continuation,
    };
    use crate::test_support::GameStateBuilder;

    fn state_advancing_agenda(interactive: bool) -> crate::state::GameState {
        let mut state = GameStateBuilder::new().build();
        state.agenda_deck = vec![
            Agenda {
                code: CardCode("_a1".into()),
                doom_threshold: 1,
            },
            Agenda {
                code: CardCode("_a2".into()),
                doom_threshold: 3,
            },
        ];
        state.agenda_index = 0;
        state.interactive_acknowledge = interactive;
        // An AdvanceReverse frame as advance_agenda would push it (leaving = a1).
        // Agenda advances are always Forced.
        state.continuations.push(Continuation::AdvanceReverse {
            deck: AdvanceDeck::Agenda,
            from: 0,
            leaving_code: CardCode("_a1".into()),
            step: AdvanceStep::AwaitAck,
            trigger: AdvanceTrigger::Forced,
        });
        state
    }

    /// An act mid-advance with the given `trigger`, `interactive_acknowledge` per
    /// the arg. Mirrors `state_advancing_agenda` for the act deck.
    fn state_advancing_act(interactive: bool, trigger: AdvanceTrigger) -> crate::state::GameState {
        let mut state = GameStateBuilder::new().build();
        state.act_deck = vec![
            Act {
                code: CardCode("_c1".into()),
                clue_threshold: 1,
            },
            Act {
                code: CardCode("_c2".into()),
                clue_threshold: 2,
            },
        ];
        state.act_index = 0;
        state.interactive_acknowledge = interactive;
        state.continuations.push(Continuation::AdvanceReverse {
            deck: AdvanceDeck::Act,
            from: 0,
            leaving_code: CardCode("_c1".into()),
            step: AdvanceStep::AwaitAck,
            trigger,
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

    /// Forced + interactive: the frame suspends with a one-option on-card
    /// `PickSingle` anchored to the agenda (the flip pick) before firing the
    /// reverse — the cursor has NOT bumped yet (#558).
    #[test]
    fn forced_interactive_advance_prompts_on_card_pick_anchored_to_the_deck() {
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
            panic!("expected the on-card advance pick, got {out:?}");
        };
        assert_eq!(request.kind, InputKind::PickSingle);
        assert_eq!(request.options.len(), 1, "a single 'Advance' option");
        assert_eq!(
            request.options[0].target,
            Some(OptionTarget::Agenda),
            "the flip pick anchors to the agenda card"
        );
        assert_eq!(
            state.agenda_index, 0,
            "cursor must NOT bump before the reverse resolves"
        );
    }

    /// Forced + interactive on the **act** deck (01110's Ghoul-Priest-defeat
    /// advance): the flip pick anchors to the act card, not the agenda (#558).
    #[test]
    fn forced_interactive_act_advance_anchors_to_the_act() {
        use crate::InputKind;
        let mut state = state_advancing_act(true, AdvanceTrigger::Forced);
        let mut events = Vec::new();
        let out = drive(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        let EngineOutcome::AwaitingInput { request, .. } = &out else {
            panic!("expected the on-card advance pick, got {out:?}");
        };
        assert_eq!(request.kind, InputKind::PickSingle);
        assert_eq!(request.options.len(), 1, "a single 'Advance' option");
        assert_eq!(
            request.options[0].target,
            Some(OptionTarget::Act),
            "the flip pick anchors to the act card"
        );
        assert_eq!(state.act_index, 0, "cursor must NOT bump yet");
    }

    /// A **terminal** act on 01110's forced-advance path pauses on the flip
    /// acknowledge too (#558), with the ending unlatched — the player reads the
    /// reverse that ends their game before the result panel replaces the board.
    /// Answering it fires that reverse, and the reverse is what latches (ADR
    /// 0013). The agenda-deck mirror is proven end-to-end in
    /// `scenarios/tests/the_gathering_resolutions.rs`.
    #[test]
    fn a_terminal_act_pauses_on_the_flip_acknowledge_before_its_reverse_ends_the_scenario() {
        use crate::scenario::{ResolutionId, ScenarioEnding};
        use crate::state::{Act, InvestigatorId};
        use crate::test_support::{terminal_code, test_investigator};
        crate::test_support::install_test_registry();
        let mut state = state_advancing_act(true, AdvanceTrigger::Forced);
        // One act, and it is the one advancing — so it is the terminal one.
        state.act_deck = vec![Act {
            code: terminal_code(1),
            clue_threshold: 0,
        }];
        state
            .investigators
            .insert(InvestigatorId(1), test_investigator(1));
        state.turn_order = vec![InvestigatorId(1)];
        match state.continuations.last_mut() {
            Some(Continuation::AdvanceReverse { leaving_code, .. }) => {
                *leaving_code = terminal_code(1);
            }
            other => unreachable!("fixture puts the frame on top, got {other:?}"),
        }

        let mut events = Vec::new();
        let out = drive(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        let EngineOutcome::AwaitingInput { request, .. } = &out else {
            panic!("expected the on-card advance pick, got {out:?}");
        };
        assert_eq!(request.options.len(), 1, "a single 'Advance' option");
        assert_eq!(
            request.options[0].target,
            Some(OptionTarget::Act),
            "the flip pick anchors to the act card"
        );
        assert!(
            state.ending.is_none(),
            "the ending waits for the reverse the player is being asked to read"
        );

        let resumed = resume(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &InputResponse::PickSingle(OptionId(0)),
        );
        assert_eq!(resumed, EngineOutcome::Done);
        // The flip is answered; drop the flag so the reverse's own #466 forced
        // acknowledge doesn't suspend this unit test. That second acknowledge is
        // asserted end-to-end in `scenarios/tests/the_gathering_resolutions.rs`;
        // what is under test here is that the ending lands *after* the flip.
        state.interactive_acknowledge = false;
        crate::engine::dispatch::drive(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            EngineOutcome::Done,
        );
        assert_eq!(
            state.ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(1))),
            "the reverse is what ended the scenario"
        );
        assert_eq!(state.act_index, 0, "terminal: the cursor never bumped");
    }

    /// A terminal card whose reverse reaches no ending fails **loudly**, at the
    /// card rather than as a cursor running off the end of the deck (ADR 0013).
    /// `_a1` is the only agenda here and no registry is installed, so its reverse
    /// fires nothing — the shape a scenario author ships by forgetting the
    /// `reach_resolution` on the last card's reverse.
    #[test]
    #[should_panic(expected = "terminal agenda 0 finished without an ending latched")]
    fn a_terminal_card_whose_reverse_reaches_no_ending_panics() {
        let mut state = state_advancing_agenda(false);
        state.agenda_deck.truncate(1);
        let mut events = Vec::new();
        let _ = crate::engine::dispatch::drive(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            EngineOutcome::Done,
        );
    }

    /// Deliberate + interactive: the advance was already the player's choice, so
    /// the frame skips the ack and drives straight to `FireReverse` — no pause,
    /// cursor not yet bumped (#558). Single-step `drive` so the frame doesn't run
    /// all the way to Finalize (no registry ⇒ the reverse is a no-op).
    #[test]
    fn deliberate_interactive_advance_skips_the_ack() {
        let mut state = state_advancing_act(true, AdvanceTrigger::Deliberate);
        let mut events = Vec::new();
        let out = drive(&mut Cx {
            state: &mut state,
            events: &mut events,
        });
        assert_eq!(out, EngineOutcome::Done, "deliberate: no pause");
        assert!(
            matches!(
                state.continuations.last(),
                Some(Continuation::AdvanceReverse {
                    step: AdvanceStep::FireReverse,
                    ..
                })
            ),
            "cursor moved straight past AwaitAck to FireReverse: {:?}",
            state.continuations.last()
        );
        assert_eq!(state.act_index, 0, "cursor not bumped until Finalize");
    }
}
