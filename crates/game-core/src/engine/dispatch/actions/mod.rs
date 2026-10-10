//! The basic actions (`glossary/Action.md`), one module each: Draw,
//! Resource, Move, Investigate, Fight, Engage and Evade. Each is taken through
//! [`take`], which decides whether the investigator may take it, pays for it,
//! and decides whether it provokes attacks of opportunity. A non-fast play and
//! an action-cost ability are taken through it too.
//!
//! Each module names its parts relative to itself, so a caller reads
//! `fight::handle`: `handle` is the turn action's handler, `perform` the
//! action's effect once it is taken, and `after_test` the after-test step of an
//! action that tests a skill.
//!
//! Each action that takes a target or destination (Move, Investigate, Fight,
//! Engage, Evade) owns one `candidates` function: what its rules scope
//! accepts. The handler, the turn menu, the designator gate and the evaluator
//! read it rather than re-deriving the set. Fight and Evade also own a
//! `has_malformed_value` predicate, which the handler rejects on and the menu
//! filters on. Between them, the menu can neither offer what the handler
//! rejects nor hide what it accepts.

use std::borrow::Cow;

pub(super) mod draw;
pub(crate) mod engage;
pub(crate) mod evade;
pub(crate) mod fight;
pub(crate) mod investigate;
pub(crate) mod r#move;
pub(super) mod resource;
pub(crate) mod take;

#[cfg(test)]
mod tests;

/// Why a designated `designator` (**Evade** or **Move**) rejects: it is not
/// implemented.
///
/// TODO(#818): neither `ActionDesignator` variant carries a modification yet,
/// and no card the build compiles prints either, so the engine says so rather
/// than performing a guess. Each designator's module says why its own payload
/// has no shape.
///
/// Read pre-cost by `designator::can_perform` and again by the evaluator's
/// perform dispatch, so the two sites share one wording.
pub(crate) fn designated_unimplemented(designator: &str) -> Cow<'static, str> {
    format!(
        "a designated {designator} is not implemented: no card the build compiles declares \
         one, so the modification it would carry has no shape yet (TODO(#818))"
    )
    .into()
}
