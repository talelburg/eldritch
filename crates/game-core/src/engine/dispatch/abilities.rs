//! Activated-ability dispatch handlers.

use std::borrow::Cow;
use std::collections::BTreeMap;

use card_dsl::dsl::{ActionDesignator, Cost, Effect, Trigger, UsageLimit};

use crate::card_registry::CardRegistry;
use crate::engine::board::{self, NotLeft};
use crate::engine::dispatch::{
    actions::take::{self, ActionDescription},
    combat, hand_discard_cost, initiation,
    legality::{self, ActivateCheckResult},
    uses::{self, SpendUsesError},
};
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{abilities_in_effect, ability_source, Cx};
use crate::event::Event;
use crate::state::{
    AbilityAddress, AbilitySource, ActionResolutionFrame, ActionResume, CandidateSource, CardCode,
    CardInPlay, CardInstanceId, GameState, Investigator, InvestigatorId, UseKind,
};

/// Handler for `TurnAction::ActivateAbility`: validates the activation, pays
/// its costs, announces it and resolves it.
///
/// [`legality::check_activate_ability`] validates that the acting investigator
/// can reach the named [`AbilitySource`], that the indexed ability is
/// activated, and that every cost is payable. The handler then pays the costs,
/// emits [`Event::AbilityActivated`], and resolves the ability: **its
/// designated action first, then its residual effect**
/// ([`push_activation_resolution`]). `glossary/Ability.md`: *"Activating such an
/// ability performs the designated action as described in the rules, but
/// modified in the manner described by the ability."*
///
/// # Action cost
///
/// An ability with an action cost is **taken as an action** ([`take::take`]).
/// The step pays its actions, this handler's hook pays the other costs and
/// announces, and the step then decides from the printed **action designator**
/// whether it provokes attacks of opportunity. If it does, both halves wait on
/// an `ActionResolution` frame and run on resume ([`resume_activate_ability`]).
///
/// A zero-action ability takes no action, so it pays, announces and pushes its
/// resolution at once. With a [`Cost::TakeHarm`] it parks both halves on an
/// `ActionResolution` frame and deals the harm on top, so the harm and every
/// sequence it opens resolve first: Appendix I pays *"the cost(s)"* at step 2,
/// before the effect at steps 3–4. That frame's gate is the one attacks of
/// opportunity use, so a harm that eliminates the controller suppresses the
/// effect, as Dynamite Blast 01024's ruling has it for an attack of opportunity
/// (<https://arkhamdb.com/card/01024>): *"If the enemies' attacks defeat you,
/// the effects won't resolve."* Validation refuses a [`Cost::TakeHarm`] or a
/// [`Cost::DiscardCardFromHand`] on an ability with an action cost.
///
/// # When a zero-action ability may be used
///
/// The Rules Reference's free triggered ability *"does not cost an action and
/// may be used during any player window"* (`glossary/Triggered_Abilities.md`).
/// So validation permits one when the acting investigator is the active
/// investigator during the Investigation phase, or when an open window's
/// `fast_actors` permits them. A moment inside another ability's resolution is
/// not itself a window. Asked whether Lola Hayes may switch roles while another
/// ability resolves, the official FAQ answers *"As long as it is during a
/// \[fast\] player window, yes."*
/// (`data/official-faq/Frequently_Asked_Questions.md`), so an empty
/// `open_windows` with a resolution in flight is a refusal, not a missing case.
/// It binds `[fast]` player abilities only: a Forced ability nests without a
/// window, and the forced path implements that separately.
pub(super) fn activate_ability(
    cx: &mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    address: &AbilityAddress,
) -> EngineOutcome {
    activate(cx, investigator, source, address, None)
}

/// [`activate_ability`] again, once the player has answered the FAQ 1.10
/// choice its take step put: paid for with the `chosen`th additional
/// action it offered. Nothing was paid when the choice was put, so this is the
/// whole activation, validation included.
pub(super) fn activate_ability_paid_with(
    cx: &mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    address: &AbilityAddress,
    chosen: usize,
) -> EngineOutcome {
    activate(cx, investigator, source, address, Some(chosen))
}

fn activate(
    cx: &mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    address: &AbilityAddress,
    chosen_additional_action: Option<usize>,
) -> EngineOutcome {
    let ActivateCheckResult {
        candidate,
        action_cost,
        designator,
        costs,
        effect,
        usage_limit,
    } = match legality::check_activate_ability(cx.state, cx.cards, investigator, source, address) {
        Ok(r) => r,
        Err(reason) => return EngineOutcome::Rejected { reason },
    };

    // Mutate. Every cost but the actions, then the announcement.
    let pay_and_announce = |cx: &mut Cx| -> Result<(), Cow<'static, str>> {
        // Count the use before the costs are paid rather than after: a
        // `DiscardSelf` cost, or `SpendUses` emptying a `discard_when_empty`
        // asset, takes the source — and the per-instance counter with it — out
        // of play mid-payment. Within this one `apply` the order is
        // unobservable: nothing between here and step 3 reads the counter, and a
        // payment that rejects snapshot-restores the whole activation, the
        // recorded use included. A use on an instance that then leaves
        // play is correctly lost — `glossary/Limits_and_Maximums.md`: *"If a
        // card leaves play and re-enters play during the same period, the card
        // is considered to be bringing a new instance of the ability to the
        // game."*
        initiation::record_initiation(cx.state, &candidate, usage_limit);
        pay_activation_costs(cx, investigator, source, &candidate.code, &costs)?;
        // The event names the **source**, not a card instance: the location and
        // enemy kinds and the act and agenda kinds have no
        // `CardInstanceId`. Consumers that want the instance ask
        // `source.instance()` and handle the `None`.
        cx.events.push(Event::AbilityActivated {
            investigator,
            source,
            code: candidate.code.clone(),
            address: address.clone(),
        });
        Ok(())
    };

    // A zero-action ability is not an action, so it takes none: push both
    // halves for the drive loop — no enclosing frame, no post-logic.
    if action_cost == 0 {
        if let Err(reason) = pay_and_announce(cx) {
            return EngineOutcome::Rejected { reason };
        }
        let Some((damage, horror)) = harm_cost(&costs) else {
            let eval_ctx = EvalContext::for_controller_with_source(investigator, source);
            push_activation_resolution(cx, designator.as_ref(), &effect, eval_ctx);
            // A hand-discard cost's choice goes above the effect, so it is paid
            // first. Validation refuses it beside a harm cost, so it is only
            // ever parked here.
            hand_discard_cost::park(cx, investigator, &candidate.code, &costs);
            return EngineOutcome::Done;
        };
        cx.state.continuations.push(ActionResolutionFrame {
            investigator,
            resume: ActionResume::ActivateAbility {
                source,
                designator,
                effect,
            },
        });
        return combat::deal_harm(cx, investigator, damage, horror);
    }

    // An action-cost ability is taken as an action. The step pays the actions,
    // the hook pays the rest and announces, and the step then either parks the
    // resolution behind the attacks of opportunity or performs it now.
    let mut description =
        ActionDescription::activate(source, address.clone(), action_cost, designator.clone());
    if let Some(chosen) = chosen_additional_action {
        description = description.paid_with_chosen_additional_action(chosen);
    }
    take::take(cx, investigator, &description, |cx| {
        pay_and_announce(cx)?;
        Ok(ActionResume::ActivateAbility {
            source,
            designator,
            effect,
        })
    })
}

/// The damage and horror an ability's [`Cost::TakeHarm`] costs total, or `None`
/// when it declares none.
fn harm_cost(costs: &[Cost]) -> Option<(u8, u8)> {
    costs
        .iter()
        .filter_map(|cost| match cost {
            Cost::TakeHarm { damage, horror } => Some((*damage, *horror)),
            _ => None,
        })
        .reduce(|(d1, h1), (d2, h2)| (d1.saturating_add(d2), h1.saturating_add(h2)))
}

/// Push an activated ability's two halves for the global drive loop, in
/// resolution order: the **designated action** it performs, then the
/// residual `effect` its text prints beside the bold word.
///
/// Continuation frames are LIFO, so they go on in reverse. Every designated
/// ability implemented today has an empty residual — a `Seq` of nothing, which
/// pops without running anything — so the observable behaviour is the
/// designated action alone; an undesignated ability is its effect alone.
///
/// A residual cannot contradict the bold word: [`Effect`](card_dsl::dsl::Effect)
/// has no fight or investigation of its own, so no effect tree can re-root a
/// designated action into a different one.
fn push_activation_resolution(
    cx: &mut Cx,
    designator: Option<&ActionDesignator>,
    effect: &Effect,
    eval_ctx: EvalContext,
) {
    evaluator::push_effect(cx, effect, eval_ctx);
    if let Some(designator) = designator {
        evaluator::push_designated_action(cx, designator, eval_ctx);
    }
}

/// Run a parked activated ability's `effect` after its `AoO` loop completes.
/// The actor-`Active` re-validation gate has already run in
/// [`resume_action_resolution`](super::resume_action_resolution); the source
/// may have self-discarded as a cost (so we run the snapshotted `effect`, not a
/// re-resolution by instance), with `instance_id` only seeding the eval
/// context's source.
///
/// TODO(#417): unlike the basic-action resumes
/// (`investigate::perform` etc., which return `Done` to *suppress*
/// gracefully when their target precondition has lapsed), this pushes the
/// effect for the drive loop with no suppression gate. An effect that returns
/// [`EngineOutcome::Rejected`] on a lapsed precondition (`Effect::Heal` on
/// First Aid 01019) is snapshot-restored by `apply()` — rolling back the
/// *whole* activation (the `AoO` damage + the spent cost) rather than
/// suppressing the primary only. Unreachable in the corpus: for the actor to
/// survive the `Active` gate yet have a lapsed precondition, an `AoO` reaction
/// would have to relocate it / unreveal the location without defeating it, and
/// no corpus reaction (Dodge cancel, Guard Dog soak) does that. Give this the
/// basic actions' suppress-on-lapse shape when a board-changing `AoO` reaction
/// lands.
pub(super) fn resume_activate_ability(
    cx: &mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    designator: Option<&ActionDesignator>,
    effect: &Effect,
) -> EngineOutcome {
    let eval_ctx = EvalContext::for_controller_with_source(investigator, source);
    push_activation_resolution(cx, designator, effect, eval_ctx);
    EngineOutcome::Done
}

/// Pay every payment cost of an activated ability beyond its actions, which
/// the step pays ([`take::take`]) — and every cost of a reaction ability, which
/// `initiation::initiate` pays through here too. Mutates state in
/// place and pushes the matching events.
/// Caller has already validated that every cost was payable *at validation
/// time*; a cost can still fail here by outliving its own source, which is
/// what the `Err` return carries.
///
/// # Addressing the source
///
/// Every source-referencing cost (`Exhaust`, `SpendUses`, `DiscardSelf`)
/// re-resolves the source by its [`CardInstanceId`] at the moment it is paid,
/// rather than indexing a position cached during validation. A cost can
/// remove the source mid-payment — `SpendUses` depleting a `discard_when_empty`
/// asset, or `DiscardSelf` — and a cached position would then address whichever
/// card slid down into the vacated slot, silently exhausting or draining a
/// different card. Resolving by identity cannot: the source is simply absent, so
/// a later source-referencing cost returns `Err` and the whole activation is
/// rejected. `apply_via` snapshot-restores state, events and the RNG position on
/// rejection, so a mid-payment `Err` leaves the board untouched.
pub(super) fn pay_activation_costs(
    cx: &mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    source_code: &CardCode,
    costs: &[Cost],
) -> Result<(), Cow<'static, str>> {
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
                let instance_id =
                    require_source_reachable(cx, investigator, source, cost, source_code)?
                        .instance_id;
                // The one uses-spending path. If this empties a
                // `discard_when_empty` pool (First Aid 01019), the card's
                // depletion discard is a constant ability with no point of
                // initiation, so it fires right here, mid-payment, not after
                // the ability resolves.
                //
                // Like `Cost::DiscardSelf`, that removes the source mid-payment,
                // safely: a later source-referencing cost re-resolves by
                // identity, finds nothing, and rejects the whole activation
                // rather than addressing a shifted neighbour.
                match uses::spend_uses(cx, investigator, instance_id, *kind, *count) {
                    Ok(()) => {}
                    Err(SpendUsesError::Undiscardable) => {
                        return Err(undiscardable_source(investigator, instance_id));
                    }
                    Err(SpendUsesError::NotInPlay) => {
                        unreachable!("require_source_reachable just found the source")
                    }
                }
            }
            Cost::DiscardSelf => {
                // Unreachable today: `reject_incompatible_costs` refuses
                // `DiscardSelf` alongside `Exhaust`/`SpendUses` at validation, so
                // nothing can have removed the source before this arm runs. Kept
                // so that, if that validation rejection is ever lifted, a missing
                // source rejects with a reason naming the cost rather than
                // reaching the discard at all.
                let instance_id =
                    require_source_reachable(cx, investigator, source, cost, source_code)?
                        .instance_id;
                discard_source(cx, investigator, instance_id)?;
            }
            // Neither is paid inline. A hand-discard is paid through a choice:
            // the caller parks its frame above the effect once every other cost
            // is paid (`hand_discard_cost::park`). The harm is dealt through the
            // deal frame once every other cost is paid and the ability
            // announced, by `activate_ability` (`harm_cost`); a triggered
            // ability carrying one is refused at the initiation gate, so never
            // reaches here. TODO(#1081): pay `TakeHarm` on triggered abilities.
            Cost::DiscardCardFromHand | Cost::TakeHarm { .. } => {}
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
/// to the same reachability predicate the validator used, so a cost
/// cannot reach a card the activation was never allowed to name.
fn require_source_reachable<'a>(
    cx: &'a mut Cx,
    investigator: InvestigatorId,
    source: AbilitySource,
    cost: &Cost,
    source_code: &CardCode,
) -> Result<&'a mut CardInPlay, Cow<'static, str>> {
    ability_source::resolve_mut(cx.state, cx.cards, investigator, source).ok_or_else(|| {
        format!(
            "ActivateAbility: the {} cost needs its source {source_code} \
             ({source:?}), but an earlier cost on the same ability removed it \
             from play",
            cost_label(cost),
        )
        .into()
    })
}

/// Discard the source instance from play through [`board::discard_from_play`],
/// wherever it sits, which files it by its **owner**: an activation on
/// another investigator's threat-area card (Haunted 01098, whose ruling lets
/// any co-located investigator discard it) sends it to its owner's pile, not
/// the activator's.
///
/// The investigator card is the one reachable source that cannot be discarded —
/// it is a permanent, and nothing in the rules removes it as a cost — and the
/// exit leaves it in place, so it rejects. Deliberately unlifted with no
/// tracking issue (YAGNI, as with `reject_incompatible_costs`); whoever prints
/// such a card files one.
///
/// A source that cannot leave play right now cannot pay the cost either
/// (`glossary/Cannot.md`: *"The word "cannot" is absolute, and cannot be
/// countermanded by other abilities."*). No corpus card prints both.
fn discard_source(
    cx: &mut Cx,
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
) -> Result<(), Cow<'static, str>> {
    match board::discard_from_play(cx, instance_id) {
        Ok(_) => Ok(()),
        Err(NotLeft::NotInPlay) => Err(undiscardable_source(investigator, instance_id)),
        Err(NotLeft::Refused) => Err(format!(
            "ActivateAbility: {investigator:?}'s source {instance_id:?} cannot leave play, \
             so it cannot be discarded as a cost",
        )
        .into()),
    }
}

/// The rejection for a cost that must discard its source when the source is
/// in no zone a discard can take it from — `DiscardSelf`, or a `SpendUses`
/// that empties a `discard_when_empty` pool.
fn undiscardable_source(
    investigator: InvestigatorId,
    instance_id: CardInstanceId,
) -> Cow<'static, str> {
    format!(
        "ActivateAbility: {investigator:?}'s source {instance_id:?} is in no zone a discard \
         can remove it from (today only the investigator card, which is a permanent), so it \
         cannot be discarded as a cost",
    )
    .into()
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
        Cost::TakeHarm { .. } => "take-harm",
    }
}

/// The parts of an [`Ability`](card_dsl::dsl::Ability) the activation path needs,
/// lifted out of the registry entry.
///
/// `usage_limit` rides along because the validator has to answer *"can this
/// source record a use at all"* before any cost is paid — a location cannot,
/// and the rejection for that is `reject_untrackable_usage_limit`.
#[derive(Debug, Clone)]
pub(super) struct ActivatedAbility {
    /// Actions the activation costs; `0` for a `[free]` ability.
    pub(super) action_cost: u8,
    /// The bold action designator the ability prints, if any — which is what
    /// **performs the action**, carrying the modification the printed text
    /// describes.
    pub(super) designator: Option<ActionDesignator>,
    /// The ability's payment costs, in printed order.
    pub(super) costs: Vec<Cost>,
    /// The effect to resolve once every cost is paid.
    pub(super) effect: Effect,
    /// The *"Limit X per \[period\]"* cap, if the card prints one.
    pub(super) usage_limit: Option<UsageLimit>,
}

/// Resolve the activated ability `address` names on the card behind `source`,
/// returning its [`ActivatedAbility`] or the rejection reason.
///
/// Split out so [`activate_ability`] stays under the function-size
/// lint, and to mirror [`resolve_play_target`]'s role for
/// [`play_card`].
pub(super) fn resolve_activated_ability(
    state: &GameState,
    cards: &CardRegistry,
    source: AbilitySource,
    code: &CardCode,
    address: &AbilityAddress,
) -> Result<ActivatedAbility, EngineOutcome> {
    // Re-derived from the address, against the side in effect and
    // the grants currently on the board — which is what the enumerator listed
    // from, so an address means the same ability on both ends of the round
    // trip. A granted ability whose granter has left play or whose condition
    // has flipped is simply not there, and rejects like an unknown one.
    let Some(ability) = abilities_in_effect::resolve(
        state,
        cards,
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
    let Trigger::Activated { action_cost } = ability.trigger else {
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
        designator: ability.designator.clone(),
        costs: ability.costs.clone(),
        effect: ability.effect.clone(),
        usage_limit: ability.usage_limit,
    })
}

/// Validate a single [`Cost`] is currently payable against `inv` /
/// `source_exhausted`. Returns the reject reason on failure. Does
/// NOT mutate; the caller does the actual deduction after all costs
/// are checked. `cards` tells a weakness in hand, which a hand-discard cost
/// may not choose.
pub(super) fn check_cost_payable(
    cards: &CardRegistry,
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
                    "ActivateAbility: the ability's card is already exhausted; Exhaust cost \
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
        // Always payable. `DiscardSelf`: the source is in play by the activation
        // precondition (check_activate_ability located it in cards_in_play).
        // `TakeHarm`: you can always take harm, even lethal harm.
        Cost::DiscardSelf | Cost::TakeHarm { .. } => Ok(()),
        Cost::DiscardCardFromHand => hand_discard_cost::payable(cards, inv),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn spend_uses_payable_only_with_enough_of_the_named_kind() {
        let inv = test_support::test_investigator(1);
        let ammo4: BTreeMap<UseKind, u8> = [(UseKind::Ammo, 4)].into_iter().collect();
        let empty: BTreeMap<UseKind, u8> = BTreeMap::new();
        let cost = Cost::SpendUses {
            kind: UseKind::Ammo,
            count: 1,
        };
        // Enough of the right kind → payable.
        assert!(
            check_cost_payable(&test_support::TEST_REGISTRY, &cost, &inv, false, &ammo4).is_ok()
        );
        // No ammo at all → reject.
        assert!(
            check_cost_payable(&test_support::TEST_REGISTRY, &cost, &inv, false, &empty).is_err()
        );
        // Wrong kind present, no ammo → reject.
        let charges: BTreeMap<UseKind, u8> = [(UseKind::Charges, 4)].into_iter().collect();
        assert!(
            check_cost_payable(&test_support::TEST_REGISTRY, &cost, &inv, false, &charges).is_err()
        );
    }
}
