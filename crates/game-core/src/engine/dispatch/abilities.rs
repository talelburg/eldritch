//! Activated-ability dispatch handlers.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::card_registry;
use crate::dsl::{ActionDesignator, Cost, Trigger};
use crate::event::Event;
use crate::state::{
    AbilityAddress, AbilitySource, CandidateSource, CardCode, CardInPlay, CardInstanceId,
    GameState, Investigator, InvestigatorId, UseKind,
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
/// and resolves the ability — **its designated action first, then its residual
/// effect** ([`push_activation_resolution`]).
///
/// The designated action is the ability's substance for every card that prints
/// a bold word (#805): `glossary/Ability.md`, *"Activating such an ability
/// **performs the designated action** as described in the rules, but modified
/// in the manner described by the ability."* Every implemented such ability's
/// residual `effect` is empty; a non-empty one would run after.
///
/// An action-cost ability whose printed **action designator** is not on the
/// attack-of-opportunity exempt list provokes one from each engaged ready
/// enemy, fired after costs and before the action — see [`provokes_aoo`]; both
/// halves are parked on an `ActionResolution` frame and run on resume
/// ([`resume_activate_ability`]). Exempt and fast abilities resolve
/// synchronously.
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
/// The negative half — a moment *inside* another ability's resolution is not
/// itself a window, so a Fast ability needs one open at it — is not printed as
/// a rule, but the official FAQ is consistent with it. Asked whether Lola Hayes
/// may switch roles mid-resolution of another ability, it answers *"As long as
/// it is during a [fast] player window, yes."*
/// (`data/official-faq/Frequently_Asked_Questions.md`) — the permission is
/// conditioned on a window rather than granted by the moment. So an empty
/// `open_windows` with a resolution in flight is a considered refusal, not a
/// missing case. It binds **`[fast]` player abilities only**: a Forced ability
/// nests without one, which the same FAQ says outright (*"it can create a
/// nested sequence if used during another ability or a skill test, and it does
/// not need to be during a player window in order for it to occur"*) and which
/// the forced path implements separately.
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
    address: &AbilityAddress,
) -> EngineOutcome {
    let super::ActivateCheckResult {
        source_code,
        action_cost,
        surcharge_sources,
        designator,
        costs,
        effect,
        source_exhausted: _,
    } = match super::reaction_windows::check_activate_ability(
        cx.state,
        investigator,
        source,
        address,
    ) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // Mutate.
    if let Err(reason) = pay_activation_costs(
        cx,
        investigator,
        source,
        &source_code,
        action_cost,
        &surcharge_sources,
        &costs,
    ) {
        return EngineOutcome::Rejected { reason };
    }
    // The event names the **source**, not a card instance: the location and
    // enemy kinds (#708) and the act and agenda kinds (#709) have no
    // `CardInstanceId`. Consumers that want the instance ask `source.instance()`
    // and handle the `None`.
    cx.events.push(Event::AbilityActivated {
        investigator,
        source,
        code: source_code,
        address: address.clone(),
    });

    // RR p.5 "Attack of Opportunity": activating an action-cost ability while
    // engaged with a ready enemy provokes one AoO from each — *unless* it prints
    // a fight/evade/parley/resign action designator. The action cost is already spent
    // (`pay_activation_costs`), so we park the effect on an `ActionResolution`
    // frame and drive the AoO loop (which may open a Dodge cancel / Guard Dog
    // soak window), then run the effect on resume. (#361, K3.)
    if provokes_aoo(action_cost, designator.as_ref()) {
        cx.state
            .continuations
            .push(crate::state::Continuation::ActionResolution {
                investigator,
                resume: crate::state::ActionResume::ActivateAbility {
                    source,
                    designator,
                    effect,
                },
            });
        return super::combat::drive_aoo(cx, investigator);
    }

    // Fast (not an action), or an AoO-exempt designator: push both halves for
    // the drive loop (Slice D, #423) — no enclosing frame, no post-logic.
    let eval_ctx =
        EvalContext::for_controller_with_optional_source(investigator, source.instance());
    push_activation_resolution(cx, designator.as_ref(), &effect, eval_ctx);
    EngineOutcome::Done
}

/// Push an activated ability's two halves for the global drive loop, in
/// resolution order: the **designated action** it performs (#805), then the
/// residual `effect` its text prints beside the bold word.
///
/// Continuation frames are LIFO, so they go on in reverse. Every ability
/// implemented today has an empty residual — a `Seq` of nothing, which pops
/// without running anything — so the observable behaviour is the designated
/// action alone; an undesignated ability is its effect alone, exactly as
/// before.
///
/// A residual cannot contradict the bold word the way the retired
/// designator-plus-`Effect::Fight` split could: with the fight and the
/// investigation gone from [`Effect`](crate::dsl::Effect) entirely, no effect
/// tree can re-root a designated action into a different one.
fn push_activation_resolution(
    cx: &mut Cx,
    designator: Option<&ActionDesignator>,
    effect: &crate::dsl::Effect,
    eval_ctx: EvalContext,
) {
    push_effect(cx, effect, eval_ctx);
    if let Some(designator) = designator {
        crate::engine::evaluator::push_designated_action(cx, designator, eval_ctx);
    }
}

/// Whether activating an ability with this `action_cost` and `designator`
/// provokes an attack of opportunity. `glossary/Attack_of_Opportunity.md`,
/// verbatim:
///
/// > Each time an investigator is engaged with one or more ready enemies and
/// > takes an action other than to **fight**, to **evade**, or to activate a
/// > **parley** or **resign** ability, each of those enemies makes an attack of
/// > opportunity against the investigator...
///
/// The exempt list is four **bold action designators**
/// ([`ActionDesignator`]), so this reads the designator the ability
/// *declares* (#696). It used to match the effect root instead — exempting
/// `Effect::Fight` because every corpus weapon happens to be rooted in one —
/// which answered the wrong question twice over: a `Seq`-wrapped Fight would
/// have provoked, and Parley and Resign have no effect shape of their own, so
/// the Parlor 01115's *"\[action\] **Resign.**"* would have provoked once
/// #708 made a location's abilities reachable.
///
/// A designated **Move** or **Investigate** ability is not on the exempt list
/// and provokes exactly as its basic action does (Flashlight 01087). Fast
/// abilities (`action_cost == 0`) are not actions and never provoke — the same
/// glossary entry, added in FAQ: *"\[free\] abilities with a bold action
/// designator do not provoke attacks of opportunity."*
fn provokes_aoo(action_cost: u8, designator: Option<&ActionDesignator>) -> bool {
    action_cost > 0
        && !matches!(
            designator,
            Some(
                ActionDesignator::Fight { .. }
                    | ActionDesignator::Evade
                    | ActionDesignator::Parley
                    | ActionDesignator::Resign
            )
        )
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
    designator: Option<&ActionDesignator>,
    effect: &crate::dsl::Effect,
) -> EngineOutcome {
    let eval_ctx =
        EvalContext::for_controller_with_optional_source(investigator, source.instance());
    push_activation_resolution(cx, designator, effect, eval_ctx);
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
    surcharge_sources: &[CardInstanceId],
    costs: &[Cost],
) -> Result<(), Cow<'static, str>> {
    if action_cost > 0 {
        let inv_mut = cx
            .state
            .investigators
            .get_mut(&investigator)
            .expect("validated above");
        inv_mut.actions_remaining = inv_mut.actions_remaining.saturating_sub(action_cost);
        // The surcharge is spent, so its `first_each_round` sources are done
        // for the round — the same commit-time marking `charge_action` does for
        // a basic action (#754). Without it Frozen in Fear 01164 would surcharge
        // a designated Fight *and* the basic Fight that follows.
        inv_mut
            .action_surcharge_spent_this_round
            .extend(surcharge_sources.iter().copied());
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
    /// The bold action designator the ability prints, if any — which is what
    /// **performs the action**, carrying the modification the printed text
    /// describes (#805).
    pub(super) designator: Option<ActionDesignator>,
    /// The ability's payment costs, in printed order.
    pub(super) costs: Vec<Cost>,
    /// The effect to resolve once every cost is paid.
    pub(super) effect: crate::dsl::Effect,
    /// The *"Limit X per \[period\]"* cap, if the card prints one.
    pub(super) usage_limit: Option<crate::dsl::UsageLimit>,
}

/// Resolve the activated ability `address` names on the card behind `source`,
/// returning its [`ActivatedAbility`] or the rejection reason.
///
/// Split out so [`activate_ability`] stays under the function-size
/// lint, and to mirror [`resolve_play_target`]'s role for
/// [`play_card`].
pub(super) fn resolve_activated_ability(
    state: &GameState,
    source: AbilitySource,
    code: &CardCode,
    address: &AbilityAddress,
) -> Result<ActivatedAbility, EngineOutcome> {
    if card_registry::current().is_none() {
        return Err(EngineOutcome::Rejected {
            reason: "ActivateAbility: no card registry installed; engine cannot resolve abilities."
                .into(),
        });
    }
    // Re-derived from the address (#772), against the side in effect (#774) and
    // the grants currently on the board — which is what the enumerator listed
    // from, so an address means the same ability on both ends of the round
    // trip. A granted ability whose granter has left play or whose condition
    // has flipped is simply not there, and rejects like an unknown one.
    let Some(ability) = crate::engine::abilities_in_effect::resolve(
        state,
        CandidateSource::Ability(source),
        code,
        address,
    ) else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: {address:?} names no ability in effect on {code} \
                 (unimplemented card, or a grant that no longer applies)"
            )
            .into(),
        });
    };
    let Trigger::Activated {
        action_cost,
        ref designator,
    } = ability.trigger
    else {
        return Err(EngineOutcome::Rejected {
            reason: format!(
                "ActivateAbility: {address:?} on {code} is not an Activated \
                 trigger (got {:?})",
                ability.trigger,
            )
            .into(),
        });
    };
    Ok(ActivatedAbility {
        action_cost,
        designator: designator.clone(),
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

    /// The attack-of-opportunity exemption is exactly the four designators
    /// `glossary/Attack_of_Opportunity.md` names — **fight**, **evade**,
    /// **parley**, **resign** — and nothing else. Exhaustive over the six
    /// designators plus the undesignated case, in both action and `[free]`
    /// flavours.
    #[test]
    fn provokes_aoo_exempts_exactly_the_four_named_designators() {
        use crate::dsl::{fight, investigate};
        use ActionDesignator::{Evade, Move, Parley, Resign};

        for exempt in [fight(0u8, 0u8), Evade, Parley, Resign] {
            assert!(
                !provokes_aoo(1, Some(&exempt)),
                "{exempt:?} is on the exempt list"
            );
        }
        for provoking in [Move, investigate(0u8)] {
            assert!(
                provokes_aoo(1, Some(&provoking)),
                "{provoking:?} is not on the exempt list"
            );
        }
        // No designator at all → an ordinary activate action, which provokes.
        assert!(provokes_aoo(1, None));
        // A multi-action ability still provokes (once — the loop is the
        // caller's), per "An ability that costs more than one action only
        // provokes one attack of opportunity from each engaged enemy."
        assert!(provokes_aoo(2, None));

        // `[free]` (action_cost 0) is not an action and never provokes, "even"
        // with a bold action designator (same entry, added in FAQ).
        for designator in [
            None,
            Some(fight(0u8, 0u8)),
            Some(investigate(0u8)),
            Some(Resign),
        ] {
            assert!(
                !provokes_aoo(0, designator.as_ref()),
                "{designator:?} at cost 0"
            );
        }
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
