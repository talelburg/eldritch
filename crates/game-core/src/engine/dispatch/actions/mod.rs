//! The basic actions (`glossary/Action.md`), one module each: Draw,
//! Resource, Move, Investigate, Fight, Engage and Evade. Each is taken through
//! [`take`], which decides whether the investigator may take it, pays for it,
//! and decides whether it provokes attacks of opportunity.
//!
//! Each action that takes a target or destination (Move, Investigate, Fight,
//! Engage, Evade) owns one `candidates` function: what its rules scope
//! accepts. The handler, the turn menu, the designator gate and the evaluator
//! read it rather than re-deriving the set, so the menu can neither offer what
//! the handler rejects nor hide what it accepts.

use crate::engine::Cx;
use crate::event::Event;
use crate::state::InvestigatorId;

pub(super) mod draw;
pub(crate) mod engage;
pub(crate) mod evade;
pub(crate) mod fight;
pub(crate) mod investigate;
pub(crate) mod move_action;
pub(super) mod resource;
pub(crate) mod take;

/// Spend 1 action point from the active investigator and emit
/// `ActionsRemainingChanged`. Caller has already validated that
/// `actions_remaining >= 1`.
///
/// Only a non-fast card play still pays this way; #995 takes it through
/// [`take`] and deletes this.
pub(super) fn spend_one_action(cx: &mut Cx, investigator: InvestigatorId) {
    spend_actions(cx, investigator, 1);
}

/// Spend `n` action points from the active investigator and emit a single
/// `ActionsRemainingChanged`. Caller has already validated that
/// `actions_remaining >= n`.
pub(super) fn spend_actions(cx: &mut Cx, investigator: InvestigatorId, n: u8) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked before spend_actions");
    let new_count = inv.actions_remaining - n;
    inv.actions_remaining = new_count;
    cx.events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count,
    });
}

#[cfg(test)]
mod tests;
