//! Elusive (Rogue event, 01050).
//!
//! ```text
//! Fast. Play only during your turn.
//! Disengage from each enemy engaged with you and move to a revealed location
//!   with no enemies.
//! ```
//!
//! Three steps on play, in order:
//!
//! 1. **Disengage** — a typed fan-out, [`for_each`](dsl::for_each) over
//!    [`EnemySet::EngagedWithYou`] running [`disengage`](dsl::disengage) on
//!    each. Ruling: *"Disengaging does not exhaust an enemy."*
//! 2. **Move** — a card-local native. The candidates are every revealed
//!    location with no enemies other than your own, in id order, connecting or
//!    not (ruling: *"You can move to any revealed location with no enemies, not
//!    necessarily a connecting one."*). One candidate moves you there, several
//!    ask you to pick, none moves you nowhere (ruling: *"You must move when you
//!    play Elusive, as it is not a 'may' effect, unless there is no legal place
//!    to move to or a rule effect forbids movement."* — the Parlor's barrier is
//!    such a rule, so a location you cannot enter is no candidate). It moves
//!    you through [`NativeCx::move_investigator`], so the leave and enter
//!    timing of a move fires, and no attack of opportunity does.
//! 3. **Engage** — the disengaged enemies are still ready and unengaged where
//!    you left them. Once the card has resolved, the engagement rule applies to
//!    them (`glossary/Enemy_Engagement.md`: *"Any time a ready unengaged enemy
//!    is at the same location as an investigator, it engages that
//!    investigator, …"*): with no destination they engage you again; after a
//!    move, any investigator still at the location you left. The rule is a
//!    standing one, and the engine applies it at named points rather than as a
//!    check after every change, so this step applies it by hand; #1087 makes it
//!    a settle check.
//!
//! Ruling: *"You can play Elusive to move to another location even if you're
//! not engaged with any enemies."* So the card is offered while it can do
//! either half — engaged with an enemy, or with a destination to move to — and
//! not when it can do neither (its eligibility condition), since an effect with no
//! potential to change the game state cannot be initiated.
//!
//! The destination rule is a parameter of the candidate fn: the printed card
//! reaches any revealed location, and the taboo list's version (#1063) only
//! connecting ones.
//!
//! All rulings: <https://arkhamdb.com/card/01050>.

use card_dsl::dsl::{self, Ability, EnemySet, EnemyTarget};
use game_core::card_registry::CardRegistry;
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, Grounded, NativeCx, OptionTarget};
use game_core::state::{GameState, InvestigatorId, LocationId};

use crate::impls::CardRecord;

/// `ArkhamDB` code for the original-Core printing.
pub const CODE: &str = "01050";

/// This card's registration, listed in [`ALL`](super::ALL).
pub const CARD: CardRecord = CardRecord::new(CODE, abilities)
    .effects(&[(MOVE, move_to_destination), (ENGAGE, engage_ready_enemies)])
    .conditions(&[(CAN_RESOLVE, can_resolve)]);

/// Native tag: move to a revealed location with no enemies.
const MOVE: &str = "01050:move";
/// Native tag: the engagement rule, once the card has resolved.
const ENGAGE: &str = "01050:engage";
/// Native-condition tag, the play's eligibility: engaged with an enemy, or a
/// destination to move to.
const CAN_RESOLVE: &str = "01050:can-resolve";

/// On play: disengage from each enemy engaged with you, move, then let the
/// engagement rule catch up.
#[must_use]
pub fn abilities() -> Vec<Ability> {
    vec![dsl::on_play(dsl::seq([
        dsl::for_each(EnemySet::EngagedWithYou, dsl::disengage(EnemyTarget::Each)),
        dsl::native(MOVE),
        dsl::native(ENGAGE),
    ]))
    .with_eligibility(dsl::native_condition(CAN_RESOLVE))]
}

/// Where Elusive's text sends `investigator`: each revealed location in play,
/// in id order, that is not where they are and has no enemy at it.
/// `connecting_only` narrows it to the locations connected to theirs (the
/// taboo text, #1063). Empty when they are not at a location.
///
/// Whether a rule forbids entering one is the move's own check
/// ([`enterable_destinations`]), since it needs the card registry.
fn destinations(
    state: &GameState,
    investigator: InvestigatorId,
    connecting_only: bool,
) -> Vec<LocationId> {
    let Some(here) = state
        .investigators
        .get(&investigator)
        .and_then(|i| i.current_location)
    else {
        return Vec::new();
    };
    let connections = state
        .locations
        .get(&here)
        .map(|l| l.connections.as_slice())
        .unwrap_or_default();
    state
        .locations
        .values()
        .filter(|loc| {
            loc.id != here
                && loc.revealed
                && (!connecting_only || connections.contains(&loc.id))
                && !state
                    .enemies
                    .values()
                    .any(|e| e.current_location == Some(loc.id))
        })
        .map(|loc| loc.id)
        .collect()
}

/// [`destinations`] less those a rule forbids `investigator` to enter (the
/// Parlor 01115's barrier) — the legal places to move to.
fn enterable_destinations(
    state: &GameState,
    cards: &CardRegistry,
    investigator: InvestigatorId,
    connecting_only: bool,
) -> Vec<LocationId> {
    let mut candidates = destinations(state, investigator, connecting_only);
    candidates.retain(|&loc| engine::investigator_can_enter_location(state, cards, loc));
    candidates
}

fn move_to_destination(cx: &mut NativeCx, ctx: &EvalContext) -> EngineOutcome {
    let investigator = ctx.controller;
    let candidates = enterable_destinations(cx.state(), cx.cards(), investigator, false);
    match engine::resolve_grounded_choice(
        cx.state(),
        cx.cards(),
        ctx,
        &candidates,
        "Choose a revealed location with no enemies to move to",
        |id| OptionTarget::Location(*id),
    ) {
        Grounded::Picked(destination) => cx.move_investigator(investigator, destination),
        // No legal place to move to: the card resolves without the move.
        Grounded::Empty => EngineOutcome::Done,
        Grounded::Suspend(outcome) => outcome,
    }
}

/// Step 3: the engagement rule, applied by hand once the card has resolved.
/// The engine has no standing engagement check to catch the disengaged enemies
/// up (#1087).
fn engage_ready_enemies(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    cx.engage_ready_enemies();
    EngineOutcome::Done
}

/// Engaged with an enemy, or with somewhere to move.
///
/// TODO(#1083): reads [`destinations`], not [`enterable_destinations`]: a
/// native condition gets no card registry, so a destination a rule forbids
/// entering still counts here. That errs the conservative way — the card is offered, and its
/// move finds nowhere to go — rather than refusing a play that could resolve.
fn can_resolve(state: &GameState, ctx: &EvalContext) -> bool {
    let engaged = state
        .enemies
        .values()
        .any(|e| e.engaged_with == Some(ctx.controller));
    engaged || !destinations(state, ctx.controller, false).is_empty()
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
