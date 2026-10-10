//! The Engage basic action: engage an enemy at the investigator's location.

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{ActionResume, EnemyId, InvestigatorId};

/// Handler for `TurnAction::Engage`. Engage an enemy at the
/// investigator's location that they are not already engaged with
/// (Rules Reference p.4) — it becomes engaged with the investigator.
///
/// Validate-first: the investigator may take the action ([`take::check`]),
/// the enemy is in state, at the investigator's `current_location`, and not
/// already engaged with the investigator. Then take it ([`take::take`]). Engage
/// is not on the attack-of-opportunity exempt list; the target enemy is not
/// yet engaged so it cannot attack, but every other ready engaged enemy does.
/// If the investigator survives, [`engage_primary_effect`] runs the
/// engagement.
pub(in crate::engine::dispatch) fn engage(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let description = ActionDescription::basic(ActionKind::Engage);
    if let Err(reason) = take::check(cx.state, investigator, &description) {
        return EngineOutcome::Rejected { reason };
    }
    let inv = &cx.state.investigators[&investigator];
    // A `None` location can't host an engage (matches `investigate`'s
    // guard); without it the `enemy.current_location != inv_location`
    // check below would let a locationless investigator engage a
    // locationless enemy (`None != None == false`).
    let Some(inv_location) = inv.current_location else {
        return EngineOutcome::Rejected {
            reason: format!("Engage: {investigator:?} has no current_location to engage from")
                .into(),
        };
    };
    let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
        return EngineOutcome::Rejected {
            reason: format!("Engage: enemy {enemy_id:?} is not in state").into(),
        };
    };
    if enemy.engaged_with == Some(investigator) {
        return EngineOutcome::Rejected {
            reason: format!("Engage: {investigator:?} is already engaged with {enemy_id:?}").into(),
        };
    }
    if enemy.current_location != Some(inv_location) {
        return EngineOutcome::Rejected {
            reason: format!(
                "Engage: enemy {enemy_id:?} (at {:?}) is not at {investigator:?}'s location ({inv_location:?})",
                enemy.current_location,
            )
            .into(),
        };
    }

    take::take(cx, investigator, &description, |_| {
        Ok(ActionResume::Engage { enemy: enemy_id })
    })
}

/// The engagement half of an Engage action, run after its `AoO` loop (#293).
///
/// Re-reads the enemy from live state and re-checks the target precondition
/// (the §D primary-precondition re-check): enemy still exists, is co-located
/// with the investigator, and is not already engaged with this investigator.
/// Returns `Done` on any lapsed precondition (the engagement simply does not
/// happen — legitimately suppressed, not a state corruption).
///
/// A missing investigator map entry after the `Status::Active` gate in
/// `resume_action_resolution` is a state-corruption invariant violation and
/// must `unreachable!`-panic — absence here is impossible if the gate held.
pub(in crate::engine::dispatch) fn engage_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "engage_primary_effect: investigator {investigator:?} not in map after the \
                 Status::Active re-validation gate; this is a state-corruption invariant violation"
            )
        });
    let Some(inv_location) = inv.current_location else {
        return EngineOutcome::Done; // lapsed: investigator lost its location during the AoO
    };
    let Some(enemy) = cx.state.enemies.get(&enemy_id) else {
        return EngineOutcome::Done; // lapsed: target gone
    };
    if enemy.engaged_with == Some(investigator) || enemy.current_location != Some(inv_location) {
        return EngineOutcome::Done; // lapsed: already engaged, or no longer co-located
    }
    let enemy_mut = cx.state.enemies.get_mut(&enemy_id).expect("checked above");
    enemy_mut.engaged_with = Some(investigator);
    cx.events.push(Event::EnemyEngaged {
        enemy: enemy_id,
        investigator,
    });
    EngineOutcome::Done
}
