//! Survival Instinct (Survivor skill, 01081).
//!
//! ```text
//! Innate.
//! 1 agility icon.
//! If this skill test is successful during an evasion attempt, the evading
//! investigator may immediately disengage from each other enemy engaged with
//! him or her, and may move to a connecting location.
//! ```
//!
//! The agility icon is printed metadata. `abilities()` is the one
//! [`Trigger::OnSkillTestResolution`] ability on a success, gated by an
//! [`if_`](dsl::if_) over [`Condition::SkillTestKind`] of
//! [`SkillTestKind::Evade`] — Deduction 01039's kind gate, on resolution rather
//! than commit so that a failed test runs nothing. On any other test, or a
//! failed evasion, the card is inert.
//!
//! The ability's controller is the tester, so *"the evading investigator"* is
//! who acts even when someone else committed the card. The evasion's own
//! follow-up resolves before the card's ability, so by then the evaded enemy
//! is already disengaged and exhausted, and [`EnemySet::EngagedWithYou`] is
//! exactly *"each other enemy"*.
//!
//! Three steps, in order:
//!
//! 1. **Disengage** — one Decision between disengaging from each other enemy
//!    and not. Ruling (<https://arkhamdb.com/card/01081>): *"The clause "may
//!    immediately disengage from each other enemy" gives you the choice whether
//!    or not to disengage from "each other enemy", but doesn't allow you to
//!    pick and choose which to disengage from."* So the choice is all-or-none,
//!    a [`for_each`](dsl::for_each) over [`EnemySet::EngagedWithYou`] or
//!    nothing.
//! 2. **Move** — a second Decision between moving to a connecting location
//!    and not, asked only when the evader's location has a connection. The
//!    move is a card-local native over [`NativeCx::move_investigator`], as
//!    Elusive 01050's is, so the leave and enter timing fires and no attack of
//!    opportunity does; enemies still engaged move along. Several connecting
//!    locations ask which, and one the evader cannot enter is no candidate.
//! 3. **Engage** — enemies disengaged in step 1 are still ready. The
//!    engagement rule (`glossary/Enemy_Engagement.md`: *"Any time a ready
//!    unengaged enemy is at the same location as an investigator, it engages
//!    that investigator, …"*) applies to them once the card has resolved:
//!    without a move they engage the evader again. Elusive's step, applied by
//!    hand until #1087 makes it a settle check.
//!
//! **Each "may" declines through a native that does nothing.** A
//! [`choose_one`](dsl::choose_one) offers only the branches the change-state
//! check cannot prove inert, and an empty [`seq`](dsl::seq) is provably inert,
//! so a "don't" branch written as one would be dropped and the other branch
//! taken unasked. An opaque native is never proved inert, so the decline is
//! always offered. The other branch's liveness still filters the choice: with
//! no other enemy engaged the disengage branch is inert, the decline is the
//! one live branch, and nothing is asked.
//!
//! [`Trigger::OnSkillTestResolution`]: card_dsl::dsl::Trigger::OnSkillTestResolution

use card_dsl::dsl::{self, Ability, Condition, EnemySet, EnemyTarget, SkillTestKind, TestOutcome};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget};
use game_core::state::{GameState, LocationId};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01081";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[
        (MOVE, move_to_connecting_location),
        (DECLINE, decline),
        (ENGAGE, engage_ready_enemies),
    ])
    .conditions(&[(HAS_CONNECTION, has_connection)]);

/// Native tag: move to a connecting location.
const MOVE: &str = "01081:move";
/// Native tag: the "don't" branch of each "may".
const DECLINE: &str = "01081:decline";
/// Native tag: the engagement rule, once the card has resolved.
const ENGAGE: &str = "01081:engage";
/// Native-condition tag: the evader's location connects to another.
const HAS_CONNECTION: &str = "01081:has-connection";

/// Label for disengaging, the first "may".
const DISENGAGE_LABEL: &str = "Disengage from each other enemy engaged with you";
/// Label for declining it.
const STAY_ENGAGED_LABEL: &str = "Do not disengage";
/// Label for moving, the second "may".
const MOVE_LABEL: &str = "Move to a connecting location";
/// Label for declining it.
const STAY_LABEL: &str = "Do not move";

/// On a successful evasion: may disengage from each other enemy, may move to a
/// connecting location, then the engagement rule.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_skill_test_resolution(
        TestOutcome::Success,
        dsl::if_(
            Condition::SkillTestKind(SkillTestKind::Evade),
            dsl::seq([
                dsl::choose_one([
                    (
                        DISENGAGE_LABEL,
                        dsl::for_each(EnemySet::EngagedWithYou, dsl::disengage(EnemyTarget::Each)),
                    ),
                    (STAY_ENGAGED_LABEL, dsl::native(DECLINE)),
                ]),
                dsl::if_(
                    dsl::native_condition(HAS_CONNECTION),
                    dsl::choose_one([
                        (MOVE_LABEL, dsl::native(MOVE)),
                        (STAY_LABEL, dsl::native(DECLINE)),
                    ]),
                ),
                dsl::native(ENGAGE),
            ]),
        ),
    )]
}

/// The locations connected to the evader's, in printed connection order.
/// Empty when they are not at a location.
fn connections(state: &GameState, ctx: &EvalContext) -> Vec<LocationId> {
    state
        .investigators
        .get(&ctx.controller)
        .and_then(|i| i.current_location)
        .and_then(|here| state.locations.get(&here))
        .map(|loc| loc.connections.clone())
        .unwrap_or_default()
}

/// Whether the move is worth asking about.
///
/// Reads [`connections`] without the enter check: a native condition gets no
/// card registry. That errs the conservative way — the move is offered, and
/// finds nowhere to go — as Elusive's eligibility does.
fn has_connection(state: &GameState, ctx: &EvalContext) -> bool {
    !connections(state, ctx).is_empty()
}

fn move_to_connecting_location(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let investigator = ctx.controller;
    let mut candidates = connections(cx.state(), ctx);
    candidates.retain(|&loc| engine::investigator_can_enter_location(cx.state(), cx.cards(), loc));
    match engine::resolve_grounded_choice(
        cx.state(),
        cx.cards(),
        ctx,
        &candidates,
        "Choose a connecting location to move to",
        |id| OptionTarget::Location(*id),
    ) {
        Grounded::Picked(destination) => cx.move_investigator(investigator, destination),
        // No connecting location may be entered: nothing moves.
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(outcome) => outcome,
    }
}

/// A "may" declined: nothing happens.
fn decline(_cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    EngineOutcome::Done
}

/// Step 3: the engagement rule, applied by hand once the card has resolved
/// (#1087).
fn engage_ready_enemies(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.engage_ready_enemies();
    EngineOutcome::Done
}

#[cfg(test)]
mod tests {
    /// Catches a `CARD` record wired to the wrong code or abilities fn — the
    /// registry must dispatch CODE to this module's `abilities()`.
    #[test]
    fn registry_dispatches_to_this_modules_abilities() {
        assert_eq!(crate::abilities_for(super::CODE), Some(super::abilities()));
    }
}
