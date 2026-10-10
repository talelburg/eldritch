//! The basic actions (`glossary/Action.md`), one module each: Draw,
//! Resource, Move, Investigate, Fight, Engage and Evade. This module holds
//! the helpers they share: basic-action validation and spending actions.

use card_dsl::dsl::ActionClass;

use crate::card_registry;
use crate::engine::outcome::EngineOutcome;
use crate::engine::{evaluator, Cx};
use crate::event::Event;
use crate::state::{CardInstanceId, GameState, Investigator, InvestigatorId, Phase, Status};

pub(super) mod draw;
pub(super) mod engage;
pub(super) mod evade;
pub(crate) mod fight;
pub(crate) mod investigate;
pub(super) mod move_action;
pub(super) mod resource;

/// Validate the preconditions shared by every action-point-spending
/// basic action: Investigation phase, `investigator` is the active
/// investigator, `Status::Active`, and at least one action remaining.
/// Returns the validated investigator. `action_name` is interpolated
/// into rejection reasons; an active investigator missing from the map
/// is a state-corruption invariant and panics.
///
/// Move / Fight / Evade defer the action-point check to `charge_action`
/// (which folds in the Frozen-in-Fear surcharge), so `move_action` keeps
/// its own prefix; `fight` calls this directly then does its own co-location
/// check (#401), while `evade` reaches it via
/// [`validate_engaged_action`](evade::validate_engaged_action), which adds the
/// engagement check.
pub(crate) fn validate_basic_action<'a>(
    state: &'a GameState,
    action_name: &'static str,
    investigator: InvestigatorId,
) -> Result<&'a Investigator, EngineOutcome> {
    if state.phase != Phase::Investigation {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "{action_name} is only valid during the Investigation phase (was {:?})",
                state.phase
            )
            .into(),
        });
    }
    if state.active_investigator != Some(investigator) {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "{action_name}: {investigator:?} is not the active investigator ({:?})",
                state.active_investigator,
            )
            .into(),
        });
    }
    let inv = state.investigators.get(&investigator).unwrap_or_else(|| {
        unreachable!(
            "{action_name}: active_investigator {investigator:?} is not in the investigators \
             map; this is a state-corruption invariant violation"
        )
    });
    if inv.status != Status::Active {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "{action_name}: {investigator:?} is not Active (status {:?})",
                inv.status,
            )
            .into(),
        });
    }
    if inv.actions_remaining < 1 {
        return Err(EngineOutcome::Rejected {
            reason: format!("{action_name} requires at least 1 action point").into(),
        });
    }
    Ok(inv)
}

/// Spend 1 action point from the active investigator and emit
/// `ActionsRemainingChanged`. Caller has already validated that
/// `actions_remaining >= 1`.
pub(super) fn spend_one_action(cx: &mut Cx, investigator: InvestigatorId) {
    spend_actions(cx, investigator, 1);
}

/// Spend `n` action points from the active investigator and emit a single
/// `ActionsRemainingChanged`. Caller has already validated that
/// `actions_remaining >= n`.
pub(super) fn spend_actions(cx: &mut Cx, investigator: InvestigatorId, n: u8) {
    let inv = cx
        .state
        .investigators
        .get_mut(&investigator)
        .expect("investigator existence checked before spend_actions");
    let new_count = inv.actions_remaining - n;
    inv.actions_remaining = new_count;
    cx.events.push(Event::ActionsRemainingChanged {
        investigator,
        new_count,
    });
}

/// What one basic action costs before any surcharge. Named so the surcharge's
/// two consumers — [`action_cost`] and [`charge_action`] — state the formula
/// once between them.
const BASIC_ACTION_COST: u8 = 1;

/// The `ExtraActionCost` surcharge on `action_class` for `investigator`, plus
/// the `first_each_round` sources to mark spent once the action commits.
///
/// The registry-optional wrapper around
/// [`pending_action_surcharge`](crate::engine::evaluator::pending_action_surcharge):
/// no registry, or no card data for a code, means no surcharge. Pure, so
/// a caller can peek at the cost before deciding to pay it — which is what
/// validate-first requires of both consumers.
///
/// **Both ways of taking an action of a class read this**: the basic-action
/// handlers via [`action_cost`] / [`charge_action`], and an activated ability
/// whose bold designator names the class via
/// [`ActionDesignator::action_class`](card_dsl::dsl::ActionDesignator::action_class)
/// (#754). Sharing it is the point — a surcharge only one of the two applies is
/// the bug that made shooting a weapon cheaper than punching.
pub(crate) fn action_surcharge(
    state: &GameState,
    investigator: InvestigatorId,
    action_class: ActionClass,
) -> (u8, Vec<CardInstanceId>) {
    match card_registry::current() {
        Some(reg) => evaluator::pending_action_surcharge(state, reg, investigator, action_class),
        None => (0, Vec::new()),
    }
}

/// The action-point cost of a **basic** `action_class` for `investigator`: base
/// 1 plus any Frozen-in-Fear `ExtraActionCost` surcharge (Rules Reference;
/// #164). Pure. The enumerator uses this for Move/Fight/Evade affordability;
/// [`charge_action`] uses it then spends.
///
/// An activated ability pays its *printed* action cost plus the same surcharge
/// rather than this, since the printed cost need not be 1 — see
/// `check_activate_ability`.
pub(crate) fn action_cost(
    state: &GameState,
    investigator: InvestigatorId,
    action_class: ActionClass,
) -> u8 {
    BASIC_ACTION_COST.saturating_add(action_surcharge(state, investigator, action_class).0)
}

/// Charge the action cost for `action_class` (base 1 + any Frozen-in-Fear
/// `ExtraActionCost` surcharge): validate-first, returning `Err(Rejected)`
/// without mutating if the investigator lacks the points. On `Ok` the
/// actions are spent and the surcharge sources are marked spent for the
/// round. **Mutates on success**, so call it after every other precondition
/// for the action has passed. Falls back to cost 1 with no surcharge when
/// no registry is installed. Shared by move/fight/evade.
fn charge_action(
    cx: &mut Cx,
    investigator: InvestigatorId,
    action_class: ActionClass,
    action_name: &str,
) -> Result<(), EngineOutcome> {
    let (extra, to_mark) = action_surcharge(cx.state, investigator, action_class);
    let cost = BASIC_ACTION_COST.saturating_add(extra);
    let remaining = cx
        .state
        .investigators
        .get(&investigator)
        .map_or(0, |inv| inv.actions_remaining);
    if remaining < cost {
        return Err(EngineOutcome::Rejected {
            reason: format!("{action_name} requires {cost} action point(s)").into(),
        });
    }
    spend_actions(cx, investigator, cost);
    if let Some(inv) = cx.state.investigators.get_mut(&investigator) {
        inv.action_surcharge_spent_this_round.extend(to_mark);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
