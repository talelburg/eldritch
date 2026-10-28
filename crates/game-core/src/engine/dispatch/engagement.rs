//! Enemy engagement: the one place that writes [`Enemy::engaged_with`].
//!
//! Two exits change an engagement. [`engage`] places an enemy in an
//! investigator's threat area, first disengaging it from whoever held it, and
//! [`disengage`] (with its controller-scoped wrapper [`disengage_from`]) breaks
//! an engagement. Each logs its event alongside the write, so an engagement
//! never changes without [`Event::EnemyEngaged`] or [`Event::EnemyDisengaged`].
//!
//! The rest of the module is the framework rule that decides *who* a ready,
//! unengaged enemy engages. `glossary/Enemy_Engagement.md`: *"Any time a ready
//! unengaged enemy is at the same location as an investigator, it engages that
//! investigator, and is placed in that investigator's threat area. If there are
//! multiple investigators at the same location as a ready unengaged enemy,
//! follow the enemy's prey instructions to determine which investigator is
//! engaged."* [`candidates`] is the set that rule chooses among, read by every
//! path that applies it: spawning, a hunter's arrival, readying, relocation,
//! elimination's re-engage, and an investigator entering.
//!
//! The Engage action (`actions/engage.rs`) and an enemy that spawns engaged
//! with its drawer (`encounter.rs`) name their investigator outright and call
//! [`engage`] directly. Leaving play (`board::leave_play`) ends an engagement by
//! removing the enemy, with no [`Event::EnemyDisengaged`]; that is the
//! documented contract of the events that announce the removal.

use card_dsl::card_data::Prey;

use crate::engine::dispatch::cursor;
use crate::engine::dispatch::hunters::{self, PreyResolution};
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{Enemy, EnemyId, GameState, InvestigatorId, LocationId, Owner};

/// Engage `enemy_id` with `investigator`, logging [`Event::EnemyEngaged`].
///
/// An enemy engaged with a different investigator is disengaged from them
/// first, logging [`Event::EnemyDisengaged`] before the engagement:
/// `glossary/Engage_Action.md`, *"The enemy simultaneously disengages from the
/// previous investigator and engages the investigator performing the action."*
/// An enemy already engaged with `investigator` is left as it is, and nothing
/// is logged.
///
/// The enemy is in state: callers name one they have just found or minted
/// there, so a missing enemy is a state-corruption invariant violation.
pub(crate) fn engage(cx: &mut Cx, enemy_id: EnemyId, investigator: InvestigatorId) {
    let previous = enemy_mut(cx, enemy_id, "engage").engaged_with;
    if previous == Some(investigator) {
        return;
    }
    if previous.is_some() {
        disengage(cx, enemy_id);
    }
    enemy_mut(cx, enemy_id, "engage").engaged_with = Some(investigator);
    cx.events.push(Event::EnemyEngaged {
        enemy: enemy_id,
        investigator,
    });
}

/// Break `enemy_id`'s engagement, whoever it is engaged with, logging
/// [`EnemyDisengaged`](Event::EnemyDisengaged) with the investigator it was
/// engaged with. An unengaged enemy is left as it is, and nothing is logged.
///
/// The evasion's step (`glossary/Evade_Action.md`: *"the enemy is exhausted
/// (if it was ready) and the engagement is broken"*), elimination's step 3, and
/// a Move whose engaged enemy cannot follow. Only the enemy's engagement
/// changes; where it stands, and whether it is ready, are the caller's
/// business. A card that disengages *you* from an enemy is [`disengage_from`].
///
/// The enemy is in state, as for [`engage`].
pub(crate) fn disengage(cx: &mut Cx, enemy_id: EnemyId) {
    if let Some(investigator) = enemy_mut(cx, enemy_id, "disengage").engaged_with.take() {
        cx.events.push(Event::EnemyDisengaged {
            enemy: enemy_id,
            investigator,
        });
    }
}

/// Disengage `investigator` from `enemy_id`: [`disengage`], but only if the
/// enemy is engaged with `investigator`. An enemy engaged with someone else, or
/// with nobody, is left as it is, and nothing is logged.
///
/// The step [`Effect::Disengage`](card_dsl::dsl::Effect::Disengage) runs for
/// its controller: Elusive 01050's *"Disengage from each enemy engaged with
/// you"* breaks your engagements, not another investigator's.
///
/// The enemy is in state, as for [`engage`].
pub(crate) fn disengage_from(cx: &mut Cx, enemy_id: EnemyId, investigator: InvestigatorId) {
    if enemy_mut(cx, enemy_id, "disengage_from").engaged_with == Some(investigator) {
        disengage(cx, enemy_id);
    }
}

/// Whether `enemy`'s prey lets it move toward or engage `investigator` at all,
/// before any ranking: false only for a [`Prey::BearerOnly`] enemy and an
/// investigator other than its bearer.
///
/// `glossary/Prey.md`: *"If an enemy's prey instructions contain the word
/// "only," that enemy only moves towards and engages that investigator (as if
/// it were the only investigator in play), and ignores all other investigators
/// while moving and engaging. Other investigators may use the engage action or
/// card abilities to engage the enemy."* So the Engage action does not read
/// this. The bearer is the enemy's owner (`GLOSSARY.md`, Owner / Controller);
/// an enemy owned by no investigator has no bearer to single out and pursues
/// everyone, as [`Prey::Default`] does.
///
/// It is a filter rather than a ranking because "as if it were the only
/// investigator in play" rules the others out of *nearest* too, so a hunter
/// applies it before the distance calculation, not inside
/// [`hunters::resolve_prey`].
pub(super) fn pursues(enemy: &Enemy, investigator: InvestigatorId) -> bool {
    match (enemy.prey, enemy.owner) {
        (Prey::BearerOnly, Owner::Investigator(bearer)) => investigator == bearer,
        _ => true,
    }
}

/// The investigators a ready, unengaged `enemy_id` may engage where it stands:
/// the active investigators at its location that its prey pursues
/// ([`pursues`]), in player order. Empty for an enemy with no location.
///
/// The set the enemy's prey then narrows. Every application of the engagement
/// rule reads it, so a bearer-only enemy beside only investigators other than
/// its bearer engages no one (`glossary/Prey.md`: *"ignores all other
/// investigators while moving and engaging"*).
pub(super) fn candidates(state: &GameState, enemy_id: EnemyId) -> Vec<InvestigatorId> {
    let Some(enemy) = state.enemies.get(&enemy_id) else {
        return Vec::new();
    };
    let Some(location) = enemy.current_location else {
        return Vec::new();
    };
    cursor::active_investigators_at(state, location)
        .into_iter()
        .filter(|id| pursues(enemy, *id))
        .collect()
}

/// Engage a now-unengaged enemy with a co-located investigator per the
/// general engagement rule (`glossary/Enemy_Engagement.md`), narrowing several
/// [`candidates`] by the enemy's prey.
///
/// No-op when the enemy is exhausted (*"An exhausted unengaged enemy does not
/// engage"*) or has no location. On a prey `Tie` this engages `tied[0]`, the
/// `turn_order`-first of the tied investigators because [`candidates`] is
/// turn-order-ordered, rather than suspending for a tie-break `PickSingle` —
/// keeping every caller synchronous. TODO(#151): make the multiplayer tie an
/// interactive choice.
///
/// Callers: elimination's step-3 re-engagement, the Upkeep "engage on ready",
/// relocation, and [`engage_ready_enemies`].
///
/// Precondition: `enemy.engaged_with` is `None` on entry. An engaged enemy
/// would be re-targeted through [`engage`], which is not what any of the rule's
/// triggers mean.
pub(super) fn reengage_at_location(cx: &mut Cx, enemy_id: EnemyId) {
    let enemy = &cx.state.enemies[&enemy_id];
    if enemy.exhausted || enemy.current_location.is_none() {
        return;
    }
    let prey = enemy.prey;
    let candidates = candidates(cx.state, enemy_id);
    match hunters::resolve_prey(cx.state, cx.cards, prey, &candidates) {
        PreyResolution::None => {}
        PreyResolution::One(target) => engage(cx, enemy_id, target),
        PreyResolution::Tie(tied) => engage(cx, enemy_id, tied[0]),
    }
}

/// Apply the engagement rule to the whole board: every ready, unengaged enemy
/// at a location with an investigator engages one there, in enemy-id order,
/// through [`reengage_at_location`] (so its prey picks among several).
///
/// `glossary/Enemy_Engagement.md`: *"Any time a ready unengaged enemy is at the
/// same location as an investigator, it engages that investigator."* The engine
/// applies that rule at its triggers (entering, spawning, readying,
/// elimination); this is the trigger for a card effect that leaves enemies
/// ready and unengaged beside an investigator. First consumer: Elusive 01050,
/// which disengages the enemies engaged with you and then moves you — if it
/// cannot move you, they engage you again once it has resolved.
pub(crate) fn engage_ready_enemies(cx: &mut Cx) {
    let unengaged: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| !e.exhausted && e.engaged_with.is_none() && e.current_location.is_some())
        .map(|(id, _)| *id)
        .collect();
    for enemy_id in unengaged {
        reengage_at_location(cx, enemy_id);
    }
}

/// Auto-engage on entering: every ready (`!exhausted`), currently-unengaged
/// enemy at `location` engages `investigator`, in deterministic `EnemyId` order
/// (`glossary/Enemy_Engagement.md`: *"Any time a ready unengaged enemy is at
/// the same location as an investigator, it engages that investigator, and is
/// placed in that investigator's threat area."*, for its example *"An
/// investigator moves into the same location as it."*).
///
/// An enemy already engaged with another investigator keeps its current
/// engagement (an enemy engages only one investigator); an enemy that moved here
/// engaged with the entering investigator is skipped (already engaged). No
/// player choice is involved — every qualifying enemy engages the one investigator
/// who entered — so this stays synchronous. A bearer-only enemy engages only
/// its bearer ([`pursues`]): `glossary/Prey.md`, *"ignores all other
/// investigators while moving and engaging"*.
///
/// TODO(#653): the **Aloof** carve-out (an Aloof enemy does not automatically
/// engage) is not modelled — consistent with the spawn, Hunter, and
/// Upkeep-reengage paths, which also don't gate on Aloof. No Aloof enemy
/// appears in a currently-implemented scenario.
pub(super) fn engage_ready_enemies_on_enter(
    cx: &mut Cx,
    investigator: InvestigatorId,
    location: LocationId,
) {
    let to_engage: Vec<EnemyId> = cx
        .state
        .enemies
        .iter()
        .filter(|(_, e)| {
            e.current_location == Some(location)
                && !e.exhausted
                && e.engaged_with.is_none()
                && pursues(e, investigator)
        })
        .map(|(id, _)| *id)
        .collect();
    for enemy_id in to_engage {
        engage(cx, enemy_id, investigator);
    }
}

/// `enemy_id`'s live entry, which the caller guarantees is in state.
fn enemy_mut<'a>(cx: &'a mut Cx, enemy_id: EnemyId, caller: &str) -> &'a mut Enemy {
    cx.state.enemies.get_mut(&enemy_id).unwrap_or_else(|| {
        unreachable!(
            "{caller}: enemy {enemy_id:?} is not in state.enemies; \
             this is a state-corruption invariant violation"
        )
    })
}

#[cfg(test)]
mod tests;
