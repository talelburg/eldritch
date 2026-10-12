//! Where a movement block bites, for both sides of the table (#651, #774).
//!
//! Two predicates, deliberately not one. Barricade 01038 blocks **enemies**
//! and not investigators (*"Non-\[\[Elite\]\] enemies cannot move into attached
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
//! The **exemption filter** is the one thing the two sides do not share: it is
//! the enemy side's alone, and there is no investigator analogue. It is the
//! declaring card's own — Barricade's printed *"Non-\[\[Elite\]\]"* — carried on
//! [`Restriction::EnemyMovementBlocked`], so this module names no trait.
//!
//! # Where a restriction can be printed
//!
//! Either on the location's own card or on something attached to it, so
//! [`location_carries_restriction`] reads both. Barricade is an attachment;
//! the Parlor's barrier is printed on the location itself — on its
//! **unrevealed back**, which is why the location's own side is selected
//! through [`abilities_in_effect`](crate::engine::abilities_in_effect) rather
//! than read straight off `abilities_for`.
//!
//! Those two are not every per-location collection: #771 added a third,
//! `Location::cards_at_location`, and this module does **not** read it. See the
//! `TODO(#826)` on [`location_carries_restriction`] for why that is a choice
//! rather than an oversight.

use card_dsl::dsl::{Ability, CardFilter, Effect, Restriction, Trigger};

use crate::card_registry;
use crate::engine::abilities_in_effect;
use crate::state::{Enemy, GameState, LocationId};

/// Whether `loc` carries a constant [`Restriction`] that passes `bites` — on
/// the location's own in-effect side, or on any card attached to it.
///
/// Read the way `play_is_prohibited` reads constant restrictions: a
/// `Trigger::Constant` ability whose effect is `Effect::Restrict(r)` with
/// `bites(r)`. A predicate rather than an equality, because a restriction can
/// carry data the reader inspects (the enemy side's exemption filter).
/// `false` with no registry installed.
///
/// **TODO(#826): this does not read
/// [`Location::cards_at_location`](crate::state::Location::cards_at_location).**
/// #771 gave a location a third per-location `Vec<CardInPlay>` — cards put into
/// play *at* it under no controller — and copied the attachment arm at
/// reachability, the modifier sweep and both instance walks, but not here. It is
/// unreachable rather than latent-wrong: the zone's only occupant is Lita
/// Chantler 01117, who carries no [`Effect::Restrict`], so no board state exists
/// in which the two collections disagree about an answer anyone asks for. The
/// open question #826 settles is whether a movement restriction is
/// attachment-shaped by nature — Barricade 01038 is the corpus's only instance —
/// or whether this walk should simply chain both collections. The first card
/// that puts a restriction-carrying card into play at a location forces it.
fn location_carries_restriction(
    state: &GameState,
    loc: LocationId,
    bites: impl Fn(&Restriction) -> bool,
) -> bool {
    let carries = |abilities: &[Ability]| {
        abilities.iter().any(|a| {
            a.trigger == Trigger::Constant
                && matches!(&a.effect, Effect::Restrict(got) if bites(got))
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

/// Whether `enemy` may move into `loc`. Blocked when `loc` carries a
/// [`Restriction::EnemyMovementBlocked`] whose exemption the enemy's printed
/// card does not pass — a `None` exemption blocks every enemy. Barricade 01038
/// supplies its own *"Non-\[\[Elite\]\]"* as that exemption.
///
/// The exemption is checked by code through
/// [`card_registry::card_matches`], so with no registry installed it fails
/// closed and the enemy is blocked.
///
/// Shared by Hunter movement and forced enemy-movement effects (agenda
/// 01107's Ghoul move), so a barricade is honored consistently regardless of
/// what moves the enemy — and, per 01115's ruling above, an enemy is **not**
/// stopped by the Parlor's investigator-side barrier.
#[must_use]
pub fn enemy_can_enter_location(state: &GameState, enemy: &Enemy, loc: LocationId) -> bool {
    // Exempt only when the restriction names a filter and this enemy matches it.
    let is_exempt = |exempt: Option<&CardFilter>| {
        exempt.is_some_and(|f| card_registry::card_matches(&enemy.code, f))
    };
    let blocks_enemy = |r: &Restriction| match r {
        Restriction::EnemyMovementBlocked { exempt } => !is_exempt(exempt.as_ref()),
        _ => false,
    };
    !location_carries_restriction(state, loc, blocks_enemy)
}

/// Whether an investigator may move into `loc`. Blocked when `loc` carries
/// [`Restriction::InvestigatorMovementBlocked`] — the Parlor 01115's
/// unrevealed back.
///
/// Takes no investigator: the exemption filter is the enemy side's alone, and
/// the printed text names no investigator characteristic (*"**You** cannot
/// move into the Parlor"* is addressed to whoever is moving). Applied at both
/// of the Move action's gates — destination enumeration and its own
/// validate-first — so the barrier is enforced whether the player picks from
/// the menu or submits at the `apply` seam.
#[must_use]
pub fn investigator_can_enter_location(state: &GameState, loc: LocationId) -> bool {
    !location_carries_restriction(state, loc, |r| {
        *r == Restriction::InvestigatorMovementBlocked
    })
}
