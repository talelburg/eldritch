//! The basic actions (`glossary/Action.md`), one module each: Draw,
//! Resource, Move, Investigate, Fight, Engage and Evade. Each is taken through
//! [`take`], which decides whether the investigator may take it, pays for it,
//! and decides whether it provokes attacks of opportunity. A non-fast play and
//! an action-cost ability are taken through it too.
//!
//! Each action that takes a target or destination (Move, Investigate, Fight,
//! Engage, Evade) owns one `candidates` function: what its rules scope
//! accepts. The handler, the turn menu, the designator gate and the evaluator
//! read it rather than re-deriving the set, so the menu can neither offer what
//! the handler rejects nor hide what it accepts.

pub(super) mod draw;
pub(crate) mod engage;
pub(crate) mod evade;
pub(crate) mod fight;
pub(crate) mod investigate;
pub(crate) mod move_action;
pub(super) mod resource;
pub(crate) mod take;

#[cfg(test)]
mod tests;
