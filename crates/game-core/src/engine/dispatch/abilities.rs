//! Activated-ability dispatch handlers.

use crate::card_registry;
use crate::dsl::{Cost, Trigger};
use crate::event::Event;
use crate::state::{CardCode, CardInstanceId, Investigator, InvestigatorId};

use super::super::evaluator::{apply_effect, EvalContext};
use super::super::outcome::EngineOutcome;
use super::Cx;

/// Handler for [`PlayerAction::ActivateAbility`].
///
/// Validates the named card instance, the indexed ability's trigger,
/// and every cost-payability precondition. On success, pays every cost
/// (emitting cost events per primitive), emits [`Event::AbilityActivated`],
/// and dispatches the ability's effect through the DSL evaluator.
///
/// # Timing gate
///
/// The gate branches on `action_cost` from `Trigger::Activated`:
///
/// - **Action-cost abilities** (`action_cost > 0`): require Investigation
///   phase + active investigator + sufficient actions remaining. These consume
///   one of the investigator's limited per-turn actions.
/// - **Fast abilities** (`action_cost == 0`): per the Rules Reference, "Fast
///   abilities may be used at any player window." This handler permits them
///   when either (a) the acting investigator is the active investigator during
///   the Investigation phase, or (b) an open window's `fast_actors` scope
///   permits the acting investigator. The `open_windows` stack is pushed by
///   callers (scenario/server) when a player window opens.
///
/// # Cost coverage
///
/// - [`Cost::Resources`](crate::dsl::Cost::Resources): validates
///   wallet, deducts on payment, emits [`Event::ResourcesPaid`].
/// - [`Cost::Exhaust`](crate::dsl::Cost::Exhaust): validates source
///   not already exhausted, flips `cards_in_play[i].exhausted`,
///   emits [`Event::CardExhausted`].
/// - [`Cost::DiscardCardFromHand`](crate::dsl::Cost::DiscardCardFromHand):
///   rejects with a TODO — target-card selection needs an engine
///   `AwaitingInput` producer + `ResolveInput` dispatch. No card on
///   the roadmap uses this cost yet, so the consumer hasn't landed.
///   Test-side seam is [`ChoiceResolver`](crate::test_support::ChoiceResolver).
///
/// # State-mutation contract
///
/// Same caveat as `play_card`: costs are paid and `AbilityActivated`
/// is emitted before `apply_effect` runs, so a mid-resolution
/// rejection inside the effect leaves the costs paid. The apply
/// loop's belt-and-suspenders `events.clear()` still wipes the event
/// stream on rejection. Phase-3 in-scope effects (`GainResources`,
/// `DiscoverClue`, `Seq` of those, future `Modify`/`ThisSkillTest`
/// push) can't reject mid-flight once the standard prefix passes.
pub(super) fn activate_ability(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    ability_index: u8,
) -> EngineOutcome {
    let super::ActivateCheckResult {
        in_play_pos,
        source_code,
        action_cost,
        costs,
        effect,
        source_exhausted: _,
    } = match super::reaction_windows::check_activate_ability(
        cx.state,
        investigator,
        instance_id,
        ability_index,
    ) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // Mutate.
    pay_activation_costs(
        cx,
        investigator,
        instance_id,
        in_play_pos,
        &source_code,
        action_cost,
        &costs,
    );
    cx.events.push(Event::AbilityActivated {
        investigator,
        instance_id,
        code: source_code,
        ability_index,
    });

    let eval_ctx = EvalContext::for_controller_with_source(investigator, instance_id);
    apply_effect(cx, &effect, eval_ctx)
}

/// Pay the action cost and every payment cost of an activated
/// ability. Mutates state in place and pushes the matching events.
/// Caller has already validated that every cost is payable.
fn pay_activation_costs(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
    in_play_pos: usize,
    source_code: &CardCode,
    action_cost: u8,
    costs: &[Cost],
) {
    if action_cost > 0 {
        let inv_mut = cx
            .state
            .investigators
            .get_mut(&investigator)
            .expect("validated above");
        inv_mut.actions_remaining = inv_mut.actions_remaining.saturating_sub(action_cost);
        let new_count = inv_mut.actions_remaining;
        cx.events.push(Event::ActionsRemainingChanged {
            investigator,
            new_count,
        });
    }
    for cost in costs {
        match cost {
            Cost::Resources(n) => {
                let inv_mut = cx
                    .state
                    .investigators
                    .get_mut(&investigator)
                    .expect("validated above");
                inv_mut.resources = inv_mut.resources.saturating_sub(*n);
                cx.events.push(Event::ResourcesPaid {
                    investigator,
                    amount: *n,
                });
            }
            Cost::Exhaust => {
                cx.state
                    .investigators
                    .get_mut(&investigator)
                    .expect("validated above")
                    .cards_in_play[in_play_pos]
                    .exhausted = true;
                cx.events.push(Event::CardExhausted {
                    investigator,
                    instance_id,
                    code: source_code.clone(),
                });
            }
            Cost::DiscardCardFromHand => {
                unreachable!("DiscardCardFromHand rejected earlier in check_cost_payable")
            }
        }
    }
}

/// Resolve the activated ability at `(code, ability_index)` from the
/// installed [`card_registry`], returning its `(action_cost, costs,
/// effect)` triple or the rejection reason.
///
/// Split out so [`activate_ability`] stays under the function-size
/// lint, and to mirror [`resolve_play_target`]'s role for
/// [`play_card`].
pub(super) fn resolve_activated_ability(
    code: &CardCode,
    ability_index: u8,
) -> Result<(u8, Vec<Cost>, crate::dsl::Effect), EngineOutcome> {
    let Some(registry) = card_registry::current() else {
        return Err(EngineOutcome::Rejected {
            reason: "ActivateAbility: no card registry installed; engine cannot resolve abilities."
                .into(),
        });
    };
    let Some(abilities) = (registry.abilities_for)(code) else {
        return Err(EngineOutcome::Rejected {
            reason: format!("ActivateAbility: card {code} has no effect implementation").into(),
        });
    };
    let idx = usize::from(ability_index);
    let Some(ability) = abilities.get(idx) else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: ability_index {ability_index} out of bounds for {code} \
                 (has {} abilities)",
                abilities.len(),
            )
            .into(),
        });
    };
    let Trigger::Activated { action_cost } = ability.trigger else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: ability {ability_index} on {code} is not an Activated \
                 trigger (got {:?})",
                ability.trigger,
            )
            .into(),
        });
    };
    Ok((action_cost, ability.costs.clone(), ability.effect.clone()))
}

/// Validate a single [`Cost`] is currently payable against `inv` /
/// `source_exhausted`. Returns the reject reason on failure. Does
/// NOT mutate; the caller does the actual deduction after all costs
/// are checked.
pub(super) fn check_cost_payable(
    cost: &Cost,
    inv: &Investigator,
    source_exhausted: bool,
) -> Result<(), String> {
    match cost {
        Cost::Resources(n) => {
            if inv.resources < *n {
                return Err(format!(
                    "ActivateAbility: needs {n} resources; investigator has {}",
                    inv.resources,
                ));
            }
            Ok(())
        }
        Cost::Exhaust => {
            if source_exhausted {
                return Err(
                    "ActivateAbility: source card is already exhausted; Exhaust cost \
                     cannot be paid"
                        .to_string(),
                );
            }
            Ok(())
        }
        Cost::DiscardCardFromHand => Err(
            "TODO: Cost::DiscardCardFromHand requires AwaitingInput + ResolveInput \
             dispatch; no card uses this cost yet so the engine consumer hasn't landed."
                .to_string(),
        ),
    }
}
