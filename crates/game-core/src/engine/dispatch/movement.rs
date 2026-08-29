//! Where a movement block bites, for both sides of the table (#651, #774).
//!
//! Two predicates, deliberately not one. Barricade 01038 blocks **enemies**
//! and not investigators (*"Non-Elite enemies cannot move into attached
//! location."*); the Parlor 01115's unrevealed back blocks **investigators**
//! and not enemies (*"You cannot move into the Parlor."*). 01115's only
//! `ArkhamDB` ruling settles that the two sides really do diverge:
//!
//! > **Q:** Can enemies move into Parlor even when investigators are blocked
//! > by the barrier? **A:** Yes; in The Gathering scenario, enemies can move
//! > into The Parlor even when the investigators are blocked by the barrier.
//! > (March 2024)
//!
//! (<https://arkhamdb.com/card/01115>)
//!
//! # The shared posture
//!
//! **A block is checked against the compelled step, never baked into the
//! connection graph** (#651). `data/rules-reference/rules/glossary/Nearest.md`:
//! *"Nearest refers to the entity of the specified kind at a location that can
//! be reached in the fewest number of connections, **even if one or more of
//! those connections are blocked by another card ability**."* So distances and
//! shortest paths run on the full graph and only the resulting step is
//! filtered — `glossary/Hunter.md` and `glossary/Patrol.md` both make the
//! blocked compelled step a **non-move** rather than a detour.
//!
//! The **Elite exemption** is the one thing the two sides do not share: it is
//! the enemy side's alone, and there is no investigator analogue.
//!
//! # Where a restriction can be printed
//!
//! Either on the location's own card or on something attached to it, so
//! [`location_carries_restriction`] reads both. Barricade is an attachment;
//! the Parlor's barrier is printed on the location itself — on its
//! **unrevealed back**, which is why the location's own side is selected
//! through [`abilities_in_effect`](crate::engine::abilities_in_effect) rather
//! than read straight off `abilities_for`.

use crate::card_registry;
use crate::dsl::{Effect, Restriction, Trigger};
use crate::engine::abilities_in_effect;
use crate::state::{Enemy, GameState, LocationId};

/// Whether `loc` carries a constant [`Restriction`] `r` — on the location's
/// own in-effect side, or on any card attached to it.
///
/// Read the way `play_is_prohibited` reads constant restrictions: a
/// `Trigger::Constant` ability whose effect is exactly `Effect::Restrict(r)`.
/// `false` with no registry installed.
fn location_carries_restriction(state: &GameState, loc: LocationId, r: &Restriction) -> bool {
    let carries = |abilities: &[crate::dsl::Ability]| {
        abilities.iter().any(|a| {
            a.trigger == Trigger::Constant && matches!(&a.effect, Effect::Restrict(got) if got == r)
        })
    };
    if carries(&abilities_in_effect::location_abilities_or_empty(
        state, loc,
    )) {
        return true;
    }
    let Some(reg) = card_registry::current() else {
        return false;
    };
    let Some(location) = state.locations.get(&loc) else {
        return false;
    };
    location
        .attachments
        .iter()
        .any(|att| (reg.abilities_for)(&att.code).is_some_and(|abilities| carries(&abilities)))
}

/// Whether `enemy` may move into `loc`. Blocked only when `loc` carries
/// [`Restriction::EnemyMovementBlocked`] (Barricade 01038) **and** the enemy
/// is non-Elite (RR: movement-blockers exempt Elite).
///
/// Shared by Hunter movement and forced enemy-movement effects (agenda
/// 01107's Ghoul move), so a barricade is honored consistently regardless of
/// what moves the enemy — and, per 01115's ruling above, an enemy is **not**
/// stopped by the Parlor's investigator-side barrier.
#[must_use]
pub fn enemy_can_enter_location(state: &GameState, enemy: &Enemy, loc: LocationId) -> bool {
    enemy_is_elite(enemy)
        || !location_carries_restriction(state, loc, &Restriction::EnemyMovementBlocked)
}

/// Whether an investigator may move into `loc`. Blocked when `loc` carries
/// [`Restriction::InvestigatorMovementBlocked`] — the Parlor 01115's
/// unrevealed back.
///
/// Takes no investigator: the Elite exemption is the enemy side's alone, and
/// the printed text names no investigator characteristic (*"**You** cannot
/// move into the Parlor"* is addressed to whoever is moving). Applied at both
/// of the Move action's gates — destination enumeration and its own
/// validate-first — so the barrier is enforced whether the player picks from
/// the menu or submits at the `apply` seam.
#[must_use]
pub fn investigator_can_enter_location(state: &GameState, loc: LocationId) -> bool {
    !location_carries_restriction(state, loc, &Restriction::InvestigatorMovementBlocked)
}

/// Whether `enemy` is Elite — read from its `traits` (populated from card
/// metadata at spawn, the same field the agenda's `is_ghoul` reads). No
/// registry round-trip.
fn enemy_is_elite(enemy: &Enemy) -> bool {
    enemy.traits.iter().any(|t| t == "Elite")
}
