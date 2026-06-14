//! Resume path for the before-timing clue-discovery interrupt (C5a #236).
//!
//! `discover_clue` suspends with `clue_interrupt_pending` set when an
//! eligible `WouldDiscoverClues` reaction is controlled. On resume:
//! `Confirm` runs the interrupt card's effect (the card-local Native
//! "discard that many from self", with the replaced count threaded via
//! `EvalContext.clue_discovery_count`) and discovers nothing; `Skip`
//! performs the deferred discovery. Either way, if a skill test is
//! mid-flight, re-enter `drive_skill_test` (its continuation was
//! pre-advanced to `PostFollowUp` before the follow-up suspended).

use super::Cx;
use crate::action::InputResponse;
use crate::card_registry;
use crate::engine::evaluator::{apply_effect, perform_discovery, EvalContext};
use crate::engine::outcome::EngineOutcome;

pub(crate) fn resume_clue_interrupt(cx: &mut Cx, response: &InputResponse) -> EngineOutcome {
    let Some(pending) = cx.state.clue_interrupt_pending.take() else {
        return EngineOutcome::Rejected {
            reason: "resume_clue_interrupt: no clue interrupt pending".into(),
        };
    };
    match response {
        InputResponse::Confirm => {
            // Run the WouldDiscoverClues ability's effect (Native discard
            // from self), threading the replaced count + source instance.
            let Some(reg) = card_registry::current() else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: registry vanished".into(),
                };
            };
            let Some(inv) = cx.state.investigators.get(&pending.controller) else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: controller vanished".into(),
                };
            };
            let Some(card) = inv
                .controlled_card_instances()
                .find(|c| c.instance_id == pending.source)
            else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: source instance vanished".into(),
                };
            };
            let code = card.code.clone();
            let Some(abilities) = (reg.abilities_for)(&code) else {
                return EngineOutcome::Rejected {
                    reason: "resume_clue_interrupt: source has no abilities".into(),
                };
            };
            let effect = abilities[pending.ability_index].effect.clone();
            let mut ctx =
                EvalContext::for_controller_with_source(pending.controller, pending.source);
            ctx.clue_discovery_count = Some(pending.count);
            let outcome = apply_effect(cx, &effect, ctx);
            if !matches!(outcome, EngineOutcome::Done) {
                return outcome;
            }
        }
        InputResponse::Skip => {
            // Decline the reaction: the discovery resolves normally.
            perform_discovery(cx, pending.location, pending.count, pending.controller);
        }
        other => {
            // Restore the pending so a retry with a valid response works.
            // (The apply loop also restores state on Rejected, but be
            // explicit since we already `take()`-d.)
            cx.state.clue_interrupt_pending = Some(pending);
            return EngineOutcome::Rejected {
                reason: format!("resume_clue_interrupt: expected Confirm or Skip, got {other:?}")
                    .into(),
            };
        }
    }
    // If a skill test was mid-flight (the dominant path: Investigate's
    // follow-up discovery), resume its driver. Its continuation was
    // pre-advanced to PostFollowUp by `finish_skill_test` before the
    // follow-up suspended, so this picks up at the right step.
    if cx.state.in_flight_skill_test.is_some() {
        super::skill_test::drive_skill_test(cx)
    } else {
        EngineOutcome::Done
    }
}
