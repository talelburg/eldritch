//! The Resource basic action: gain 1 resource.

use crate::engine::dispatch::combat;
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{ActionResolutionFrame, ActionResume, InvestigatorId};

use super::{spend_one_action, validate_basic_action};

/// Handler for `TurnAction::Resource`. The basic "gain 1 resource"
/// action (Rules Reference, Investigation step 2.2.1).
///
/// Validate-first: Investigation phase, `investigator` is active and
/// `Status::Active`, `actions_remaining >= 1`. Mutate-second: spend 1
/// action, push an [`ActionResolution`] frame, and drive the
/// attack-of-opportunity loop (#293). If the investigator survives the
/// `AoO` loop, [`resource_primary_effect`] fires and gains 1 resource.
///
/// [`ActionResolution`]: crate::state::Continuation::ActionResolution
pub(in crate::engine::dispatch) fn resource_action(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    if let Err(rejection) = validate_basic_action(cx.state, "Resource", investigator) {
        return rejection;
    }

    // Mutate-second: spend the action, then park the resource gain over its
    // attack-of-opportunity loop (#293). Push the resume frame, then drive
    // the AoO. Resource is NOT on the AoO-exempt list (only Fight, Evade,
    // Parley, Resign are), so each ready engaged enemy attacks before the
    // gain resolves.
    spend_one_action(cx, investigator);
    cx.state.continuations.push(ActionResolutionFrame {
        investigator,
        resume: ActionResume::Resource,
    });
    combat::drive_aoo(cx, investigator)
}

/// The gain half of a Resource action, run after its `AoO` loop (#293).
///
/// Resource has no target precondition (unlike Move or Investigate), so
/// there is no secondary precondition re-check here. The `resume_action_resolution`
/// `Status::Active` gate upstream already guarantees the investigator is
/// present and Active; a missing map entry here is therefore a
/// state-corruption invariant violation — it must `unreachable!`-panic.
/// There is no legitimate `Done`-return inside `resource_primary_effect`:
/// it always gains 1 resource and returns `Done`.
pub(in crate::engine::dispatch) fn resource_primary_effect(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let inv_mut = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "resource_primary_effect: investigator {investigator:?} not in map after the \
                 Status::Active re-validation gate; this is a state-corruption invariant violation"
            )
        });
    inv_mut.resources = inv_mut.resources.saturating_add(1);
    cx.events.push(Event::ResourcesGained {
        investigator,
        amount: 1,
    });
    EngineOutcome::Done
}
