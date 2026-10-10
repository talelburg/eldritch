//! The Engage basic action and its candidates: engage an enemy at the
//! investigator's location.

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{ActionResume, EnemyId, GameState, InvestigatorId};

/// The enemies an Engage may target: every enemy at `investigator`'s location
/// that is not already engaged with them, in ascending [`EnemyId`] order.
/// `glossary/Engage_Action.md`: *"To engage an enemy at the same location"*,
/// including *"an enemy engaged with another investigator"*, but *"An
/// investigator cannot use the engage action to engage an enemy he or she is
/// already engaged with."*
///
/// Read by the basic action's target validation and by the turn menu. Empty for
/// a locationless investigator, so a locationless enemy is never co-located
/// with one.
pub(crate) fn candidates(state: &GameState, investigator: InvestigatorId) -> Vec<EnemyId> {
    let Some(here) = state
        .investigators
        .get(&investigator)
        .and_then(|inv| inv.current_location)
    else {
        return Vec::new();
    };
    state
        .enemies
        .iter()
        .filter(|(_, enemy)| {
            enemy.current_location == Some(here) && enemy.engaged_with != Some(investigator)
        })
        .map(|(&id, _)| id)
        .collect()
}

/// Handler for `TurnAction::Engage`. Engage an enemy at the
/// investigator's location that they are not already engaged with
/// (Rules Reference p.4) — it becomes engaged with the investigator.
///
/// Validate-first: the investigator may take the action ([`take::check`]),
/// the enemy is in state and one of the [`candidates`]. Then take it
/// ([`take::take`]). Engage is not on the attack-of-opportunity exempt list;
/// the target enemy is not yet engaged so it cannot attack, but every other
/// ready engaged enemy does.
/// If the investigator survives, [`perform`] runs the
/// engagement.
pub(in crate::engine::dispatch) fn handle(
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
    // guard). `candidates` is empty then too, but this rejects with a message
    // about the investigator standing nowhere rather than about the enemy.
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
    if !candidates(cx.state, investigator).contains(&enemy_id) {
        let reason = if enemy.engaged_with == Some(investigator) {
            format!("Engage: {investigator:?} is already engaged with {enemy_id:?}")
        } else {
            format!(
                "Engage: enemy {enemy_id:?} (at {:?}) is not at {investigator:?}'s location ({inv_location:?})",
                enemy.current_location,
            )
        };
        return EngineOutcome::Rejected {
            reason: reason.into(),
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
pub(in crate::engine::dispatch) fn perform(
    cx: &mut Cx,
    investigator: InvestigatorId,
    enemy_id: EnemyId,
) -> EngineOutcome {
    assert!(
        cx.state.investigators.contains_key(&investigator),
        "engage::perform: investigator {investigator:?} not in map after the \
         Status::Active re-validation gate; this is a state-corruption invariant violation"
    );
    // Lapsed during the AoO: the investigator lost its location, the target is
    // gone, already engaged, or no longer co-located.
    if !candidates(cx.state, investigator).contains(&enemy_id) {
        return EngineOutcome::Done;
    }
    let enemy_mut = cx.state.enemies.get_mut(&enemy_id).expect("checked above");
    enemy_mut.engaged_with = Some(investigator);
    cx.events.push(Event::EnemyEngaged {
        enemy: enemy_id,
        investigator,
    });
    EngineOutcome::Done
}
