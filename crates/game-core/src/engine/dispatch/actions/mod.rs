//! The basic actions (`glossary/Action.md`), one module each: Draw,
//! Resource, Move, Investigate, Fight, Engage and Evade. Each is taken through
//! [`take`], which decides whether the investigator may take it, pays for it,
//! and decides whether it provokes attacks of opportunity. A non-fast play and
//! an action-cost ability are taken through it too.

pub(super) mod draw;
pub(super) mod engage;
pub(super) mod evade;
pub(crate) mod fight;
pub(crate) mod investigate;
pub(super) mod move_action;
pub(super) mod resource;
pub(crate) mod take;

#[cfg(test)]
mod tests;
