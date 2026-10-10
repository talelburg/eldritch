//! The Resource basic action: gain 1 resource.

use crate::engine::dispatch::actions::take::{self, ActionDescription, ActionKind};
use crate::engine::outcome::EngineOutcome;
use crate::engine::Cx;
use crate::event::Event;
use crate::state::{ActionResume, InvestigatorId};

/// Handler for `TurnAction::Resource`. The basic "gain 1 resource"
/// action (Rules Reference, Investigation step 2.2.1).
///
/// Resource has no target, so taking it ([`take::take`]) is the whole handler.
/// Resource is not on the attack-of-opportunity exempt list, so each ready
/// engaged enemy attacks first. If the investigator survives the attacks,
/// [`perform`] gains 1 resource.
pub(in crate::engine::dispatch) fn handle(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    take::take(
        cx,
        investigator,
        &ActionDescription::basic(ActionKind::Resource),
        |_| Ok(ActionResume::Resource),
    )
}

/// The gain half of a Resource action, run after its `AoO` loop (#293).
///
/// Resource has no target precondition (unlike Move or Investigate), so
/// there is no secondary precondition re-check here. The `resume_action_resolution`
/// `Status::Active` gate upstream already guarantees the investigator is
/// present and Active; a missing map entry here is therefore a
/// state-corruption invariant violation — it must `unreachable!`-panic.
/// There is no legitimate `Done`-return inside `perform`:
/// it always gains 1 resource and returns `Done`.
pub(in crate::engine::dispatch) fn perform(
    cx: &mut Cx,
    investigator: InvestigatorId,
) -> EngineOutcome {
    let inv_mut = cx
        .state
        .investigators
        .get_mut(&investigator)
        .unwrap_or_else(|| {
            unreachable!(
                "resource::perform: investigator {investigator:?} not in map after the \
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
