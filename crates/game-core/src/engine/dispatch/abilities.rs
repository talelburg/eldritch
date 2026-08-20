//! Activated-ability dispatch handlers.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::card_registry;
use crate::dsl::{Cost, Trigger};
use crate::event::Event;
use crate::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, Investigator, InvestigatorId, UseKind,
};

use super::super::evaluator::{push_effect, EvalContext};
use super::super::outcome::EngineOutcome;
use super::Cx;

/// Handler for `TurnAction::ActivateAbility`.
///
/// Validates that the acting investigator can reach the named
/// [`AbilitySource`] (`engine::ability_source`, #707), the indexed ability's
/// trigger,
/// and every cost-payability precondition. On success, pays every cost
/// (emitting cost events per primitive), emits [`Event::AbilityActivated`],
/// and dispatches the ability's effect through the DSL evaluator.
///
/// An action-cost, non-fight ability provokes an attack of opportunity (RR
/// p.5) from each engaged ready enemy, fired after costs and before the effect
/// — see [`provokes_aoo`]; the effect is parked on an `ActionResolution` frame
/// and run on resume ([`resume_activate_ability`]). Fight and fast abilities
/// resolve their effect synchronously.
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
///   not already exhausted, flips `exhausted` on the source instance,
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
    source: AbilitySource,
    ability_index: u8,
) -> EngineOutcome {
    let super::ActivateCheckResult {
        source_code,
        action_cost,
        costs,
        effect,
        source_exhausted: _,
    } = match super::reaction_windows::check_activate_ability(
        cx.state,
        investigator,
        source,
        ability_index,
    ) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // Mutate.
    if let Err(reason) =
        pay_activation_costs(cx, investigator, source, &source_code, action_cost, &costs)
    {
        return EngineOutcome::Rejected { reason };
    }
    // The event names the **source**, not a card instance: #708's location and
    // enemy kinds have no `CardInstanceId`, and neither will #709's act and
    // agenda. Consumers that want the instance ask `source.instance()` and
    // handle the `None`.
    cx.events.push(Event::AbilityActivated {
        investigator,
        source,
        code: source_code,
        ability_index,
    });

    // RR p.5 "Attack of Opportunity": activating an action-cost ability while
    // engaged with a ready enemy provokes one AoO from each — *unless* it is a
    // fight/evade/parley/resign ability. The action cost is already spent
    // (`pay_activation_costs`), so we park the effect on an `ActionResolution`
    // frame and drive the AoO loop (which may open a Dodge cancel / Guard Dog
    // soak window), then run the effect on resume. (#361, K3.)
    if provokes_aoo(action_cost, &effect) {
        cx.state
            .continuations
            .push(crate::state::Continuation::ActionResolution {
                investigator,
                resume: crate::state::ActionResume::ActivateAbility { source, effect },
            });
        return super::combat::drive_aoo(cx, investigator);
    }

    // Fast (not an action), or an AoO-exempt Fight ability: push the effect for
    // the drive loop (Slice D, #423) — no enclosing frame, no post-logic.
    let eval_ctx =
        EvalContext::for_controller_with_optional_source(investigator, source.instance());
    push_effect(cx, &effect, eval_ctx);
    EngineOutcome::Done
}

/// Whether activating an ability with this `action_cost` and `effect` provokes
/// an attack of opportunity (RR p.5).
///
/// True iff it is an **action** (`action_cost > 0`) that is **not** a
/// fight/evade/parley/resign ability. In this engine weapons are activated
/// `Effect::Fight` abilities (Machete 01020, .45 Automatic, Roland's .38
/// Special, Knife), so those are exempt by an effect-root match. There is no
/// `Effect::Evade` and no activated parley/resign card in scope, so Fight is
/// the only exemptible activated kind — extend this (and add the variant) if a
/// `Seq`-wrapped weapon ability or an activated evade lands. Fast abilities
/// (`action_cost == 0`) are not actions and never provoke (RR p.11).
fn provokes_aoo(action_cost: u8, effect: &crate::dsl::Effect) -> bool {
    action_cost > 0 && !matches!(effect, crate::dsl::Effect::Fight { .. })
}

/// Run a parked activated ability's `effect` after its `AoO` loop completes
/// (#361). The actor-`Active` re-validation gate has already run in
/// [`resume_action_resolution`](super::resume_action_resolution); the source
/// may have self-discarded as a cost (so we run the snapshotted `effect`, not a
/// re-resolution by instance), with `instance_id` only seeding the eval
/// context's source.
///
/// TODO(#417) (richer mid-action invalidation): unlike the basic-action resumes
/// (`investigate_primary_effect` etc., which return `Done` to *suppress*
/// gracefully when their target precondition has lapsed), this pushes the
/// effect for the drive loop with no suppression gate. Some effects
/// (`Effect::Investigate` on Flashlight 01087, `Effect::Heal` on First Aid
/// 01019) return [`EngineOutcome::Rejected`] on a lapsed precondition, which `apply()`
/// snapshot-restores — rolling back the *whole* activation (the `AoO` damage +
/// the spent cost) rather than suppressing the primary only (the §D contract).
/// Unreachable in scope: for the actor to survive the `Active` gate yet have a
/// lapsed precondition, an `AoO` reaction would have to relocate it / unreveal
/// the location without defeating it, and no in-scope reaction (Dodge cancel,
/// Guard Dog soak) does that. Give this the basic actions' suppress-on-lapse
/// shape when a board-changing `AoO` reaction lands (pairs with the §D
/// "richer mid-action invalidation" hook in the keystone spec).
pub(super) fn resume_activate_ability(
    cx: &mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    effect: &crate::dsl::Effect,
) -> EngineOutcome {
    let eval_ctx =
        EvalContext::for_controller_with_optional_source(investigator, source.instance());
    push_effect(cx, effect, eval_ctx);
    EngineOutcome::Done
}

/// Pay the action cost and every payment cost of an activated
/// ability. Mutates state in place and pushes the matching events.
/// Caller has already validated that every cost was payable *at validation
/// time*; a cost can still fail here by outliving its own source, which is
/// what the `Err` return carries.
///
/// # Addressing the source
///
/// Every source-referencing cost (`Exhaust`, `SpendUses`, `DiscardSelf`)
/// re-resolves the source by its [`CardInstanceId`] at the moment it is paid,
/// rather than indexing a position cached during validation (#706). A cost can
/// remove the source mid-payment — `SpendUses` depleting a `discard_when_empty`
/// asset, or `DiscardSelf` — and a cached position would then address whichever
/// card slid down into the vacated slot, silently exhausting or draining a
/// different card. Resolving by identity cannot: the source is simply absent, so
/// a later source-referencing cost returns `Err` and the whole activation is
/// rejected. `apply_via` snapshot-restores state, events and the RNG position on
/// rejection (#161), so a mid-payment `Err` leaves the board untouched.
fn pay_activation_costs(
    cx: &mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    source_code: &CardCode,
    action_cost: u8,
    costs: &[Cost],
) -> Result<(), Cow<'static, str>> {
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
                let card = require_source_reachable(cx, investigator, source, cost, source_code)?;
                card.exhausted = true;
                let instance_id = card.instance_id;
                cx.events.push(Event::CardExhausted {
                    investigator,
                    instance_id,
                    code: source_code.clone(),
                });
            }
            Cost::SpendUses { kind, count } => {
                let card = require_source_reachable(cx, investigator, source, cost, source_code)?;
                let instance_id = card.instance_id;
                let remaining = card.uses.entry(*kind).or_insert(0);
                *remaining = remaining.saturating_sub(*count);
                let depleted = *remaining == 0;
                cx.events.push(Event::UsesSpent {
                    investigator,
                    instance_id,
                    kind: *kind,
                    amount: *count,
                });
                // Uses-depletion auto-discard (First Aid 01019). TODO(#353):
                // rules-precise timing is post-ability-resolution, and
                // effect-depletion cards (Forbidden Knowledge 01058, Grotesque
                // Statue 01071) need the check relocated there. For First Aid
                // (depletes via this cost) the SpendUses arm is observationally
                // correct.
                //
                // Like `Cost::DiscardSelf`, this removes the source mid-payment.
                // That is no longer a trap: a later source-referencing cost
                // re-resolves by identity, finds nothing, and rejects the whole
                // activation rather than addressing a shifted neighbour (#706).
                let discards_when_empty = card_registry::current()
                    .and_then(|r| (r.metadata_for)(source_code))
                    .and_then(|m| match m.kind {
                        crate::card_data::CardKind::Asset { uses, .. } => uses,
                        _ => None,
                    })
                    .is_some_and(|u| u.discard_when_empty && u.kind == *kind);
                if depleted && discards_when_empty {
                    discard_source(cx, investigator, instance_id)?;
                }
            }
            Cost::DiscardSelf => {
                // Unreachable today: `reject_incompatible_costs` refuses
                // `DiscardSelf` alongside `Exhaust`/`SpendUses` at validation, so
                // nothing can have removed the source before this arm runs. Kept
                // because the alternative on a missing source is
                // `discard_card_from_play`'s `unreachable!` — if that validation
                // rejection is ever lifted, this is the difference between a
                // rejection and a panic reachable from player input.
                let instance_id =
                    require_source_reachable(cx, investigator, source, cost, source_code)?
                        .instance_id;
                discard_source(cx, investigator, instance_id)?;
            }
            Cost::DiscardCardFromHand => {
                unreachable!("DiscardCardFromHand rejected earlier in check_cost_payable")
            }
        }
    }
    Ok(())
}

/// The card instance behind `source`, found by identity, or the rejection
/// reason for `cost` if an earlier cost in this same activation already removed
/// it from play.
///
/// The single gate every source-referencing cost passes through, so "the source
/// might be gone by now" has one answer rather than one per cost arm. Delegates
/// to the same reachability predicate the validator used (#707), so a cost
/// cannot reach a card the activation was never allowed to name.
fn require_source_reachable<'a>(
    cx: &'a mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    cost: &Cost,
    source_code: &CardCode,
) -> Result<&'a mut CardInPlay, Cow<'static, str>> {
    crate::engine::ability_source::resolve_mut(cx.state, investigator, source).ok_or_else(|| {
        format!(
            "ActivateAbility: the {} cost needs its source {source_code} \
             ({source:?}), but an earlier cost on the same ability removed it \
             from play",
            cost_label(cost),
        )
        .into()
    })
}

/// Discard the source instance from wherever it sits: a card in play goes to
/// its controller's discard pile, a threat-area card to the encounter discard
/// (`threat_area::discard_from_threat_area`).
///
/// The investigator card is the one reachable source that cannot be discarded —
/// it is a permanent, and nothing in the rules removes it as a cost — so it
/// rejects rather than reaching `discard_card_from_play`'s `unreachable!`. A
/// **panic reachable from player input must not ship**, and widening which
/// sources an activation can name (#707) is what made that branch reachable.
/// Deliberately unlifted with no tracking issue (YAGNI, as with
/// `reject_incompatible_costs`); whoever prints such a card files one.
fn discard_source(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
) -> Result<(), Cow<'static, str>> {
    let inv = cx
        .state
        .investigators
        .get(&investigator)
        .expect("validated above");
    if inv
        .cards_in_play
        .iter()
        .any(|c| c.instance_id == instance_id)
    {
        super::cards::discard_card_from_play(cx, investigator, instance_id);
        return Ok(());
    }
    if super::threat_area::discard_from_threat_area(cx, investigator, instance_id) {
        return Ok(());
    }
    Err(format!(
        "ActivateAbility: {investigator:?}'s source {instance_id:?} is in no zone a discard \
         can remove it from (today only the investigator card, which is a permanent), so it \
         cannot be discarded as a cost",
    )
    .into())
}

/// Prose name for a [`Cost`] in a rejection reason. Reasons reach the client, so
/// they read as sentences rather than as `Debug`-printed DSL variants.
fn cost_label(cost: &Cost) -> &'static str {
    match cost {
        Cost::Resources(_) => "resource",
        Cost::Exhaust => "exhaust",
        Cost::SpendUses { .. } => "spend-uses",
        Cost::DiscardSelf => "discard-self",
        Cost::DiscardCardFromHand => "discard-a-card-from-hand",
    }
}

/// The parts of an [`Ability`](crate::dsl::Ability) the activation path needs,
/// lifted out of the registry entry.
///
/// `usage_limit` rides along because the validator has to answer *"can this
/// source record a use at all"* before any cost is paid — a location cannot
/// (#699), and the rejection for that is `reject_untrackable_usage_limit`.
#[derive(Debug, Clone)]
pub(super) struct ActivatedAbility {
    /// Actions the activation costs; `0` for a `[free]` ability.
    pub(super) action_cost: u8,
    /// The ability's payment costs, in printed order.
    pub(super) costs: Vec<Cost>,
    /// The effect to resolve once every cost is paid.
    pub(super) effect: crate::dsl::Effect,
    /// The *"Limit X per \[period\]"* cap, if the card prints one.
    pub(super) usage_limit: Option<crate::dsl::UsageLimit>,
}

/// Resolve the activated ability at `(code, ability_index)` from the
/// installed [`card_registry`], returning its [`ActivatedAbility`]
/// or the rejection reason.
///
/// Split out so [`activate_ability`] stays under the function-size
/// lint, and to mirror [`resolve_play_target`]'s role for
/// [`play_card`].
pub(super) fn resolve_activated_ability(
    code: &CardCode,
    ability_index: u8,
) -> Result<ActivatedAbility, EngineOutcome> {
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
    Ok(ActivatedAbility {
        action_cost,
        costs: ability.costs.clone(),
        effect: ability.effect.clone(),
        usage_limit: ability.usage_limit,
    })
}

/// Validate a single [`Cost`] is currently payable against `inv` /
/// `source_exhausted`. Returns the reject reason on failure. Does
/// NOT mutate; the caller does the actual deduction after all costs
/// are checked.
pub(super) fn check_cost_payable(
    cost: &Cost,
    inv: &Investigator,
    source_exhausted: bool,
    source_uses: &BTreeMap<UseKind, u8>,
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
        Cost::SpendUses { kind, count } => {
            let remaining = source_uses.get(kind).copied().unwrap_or(0);
            if remaining < *count {
                return Err(format!(
                    "ActivateAbility: needs {count} {kind:?} use(s); source has {remaining}",
                ));
            }
            Ok(())
        }
        // Source is in play by the activation precondition (check_activate_ability
        // located it in cards_in_play), so it is always payable.
        Cost::DiscardSelf => Ok(()),
        Cost::DiscardCardFromHand => Err(
            "TODO: Cost::DiscardCardFromHand requires AwaitingInput + ResolveInput \
             dispatch; no card uses this cost yet so the engine consumer hasn't landed."
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixtures::test_investigator;

    #[test]
    fn provokes_aoo_classifies_action_cost_and_fight_exemption() {
        use crate::dsl::{Effect, IntExpr};
        let fight = Effect::Fight {
            combat_modifier: IntExpr::Lit(0),
            extra_damage: IntExpr::Lit(0),
        };
        let non_fight = Effect::Native { tag: "heal".into() };

        // Action-cost non-fight ability → provokes.
        assert!(provokes_aoo(1, &non_fight));
        // Action-cost Fight ability → exempt (RR p.5).
        assert!(!provokes_aoo(1, &fight));
        // Fast ability (action_cost 0) → not an action, never provokes.
        assert!(!provokes_aoo(0, &non_fight));
        assert!(!provokes_aoo(0, &fight));
    }

    #[test]
    fn spend_uses_payable_only_with_enough_of_the_named_kind() {
        let inv = test_investigator(1);
        let ammo4: BTreeMap<UseKind, u8> = [(UseKind::Ammo, 4)].into_iter().collect();
        let empty: BTreeMap<UseKind, u8> = BTreeMap::new();
        let cost = Cost::SpendUses {
            kind: UseKind::Ammo,
            count: 1,
        };
        // Enough of the right kind → payable.
        assert!(check_cost_payable(&cost, &inv, false, &ammo4).is_ok());
        // No ammo at all → reject.
        assert!(check_cost_payable(&cost, &inv, false, &empty).is_err());
        // Wrong kind present, no ammo → reject.
        let charges: BTreeMap<UseKind, u8> = [(UseKind::Charges, 4)].into_iter().collect();
        assert!(check_cost_payable(&cost, &inv, false, &charges).is_err());
    }
}
