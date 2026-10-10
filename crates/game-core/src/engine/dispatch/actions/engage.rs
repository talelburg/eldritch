//! The Engage basic action: engage an enemy at the investigator's location.

use crate::engine::dispatch::combat;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{ActionResolutionFrame, ActionResume, EnemyId, InvestigatorId};

use super::{spend_one_action, validate_basic_action};

/// Handler for `TurnAction::Engage`. Engage an enemy at the
/// investigator's location that they are not already engaged with
/// (Rules Reference p.4) — it becomes engaged with the investigator.
///
/// Validate-first: Investigation phase, active + `Status::Active`,
/// `actions_remaining >= 1`, enemy in state, enemy at the investigator's
/// `current_location`, not already engaged with the investigator.
/// Mutate-second: spend 1 action, then park the engagement over its
/// attack-of-opportunity loop (#293). The target enemy is not yet engaged
/// so it cannot `AoO`; only OTHER ready engaged enemies do. If the
/// investigator survives, [`engage_primary_effect`] runs the engagement.
///
/// The `AoO` loop now runs as an [`ActionResolution`] frame (#293): the
/// frame is pushed, then [`combat::drive_aoo`] drives the loop.
///
/// [`ActionResolution`]: crate::state::Continuation::ActionResolution
pub(in crate::engine::dispatch) fn engage(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    let inv = match validate_basic_action(cx.state, "Engage", investigator) {
        Ok(inv) => inv,
        Err(rejection) => return rejection,
    };
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

    // Mutate-second: spend the action, then park the engagement over its
    // attack-of-opportunity loop (#293). Push the resume frame, then drive
    // the AoO. Engage is NOT on the AoO-exempt list (only Fight, Evade,
    // Parley, Resign are). The target is not yet engaged so it cannot AoO;
    // only OTHER ready engaged enemies do.
    spend_one_action(cx, investigator);
    cx.state.continuations.push(ActionResolutionFrame {
        investigator,
        resume: ActionResume::Engage { enemy: enemy_id },
    });
    combat::drive_aoo(cx, investigator)
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
