//! **The initiation gate**: whether a card may be played, or an ability
//! initiated, right now
//! (`docs/adr/0017-initiation-is-one-gate-timing-is-not-part-of-it.md`).
//!
//! Appendix I opens every play and every triggered ability on the same two
//! preliminary confirmations
//! (`data/rules-reference/rules/Appendix_I_Initiation_Sequence.md`):
//!
//! > - Check play restrictions: determine if the card can be played, or if the
//! >   ability can be initiated, at this time. (This includes verifying that the
//! >   resolution of the effect has the potential to change the game state.) If
//! >   the play restrictions are not met, abort this process.
//! > - Determine the cost (or costs, if multiple costs are required) to play the
//! >   card or initiate the ability. If it is established that the cost (taking
//! >   modifiers into account) can be paid, proceed with the remaining steps of
//! >   this sequence.
//!
//! [`check`] answers both, for every path that offers or fires something, and
//! [`record_initiation`] counts a use at step 3. Each path keeps its own *when*
//! question — timing cells, `trigger_matches`, the scans' scoping, the
//! turn/Fast matrix, slots and the action economy — and asks this module only
//! *whether*. The checks each [`InitiationKind`] gets are one table,
//! [`applies`].

use std::borrow::Cow;

use card_dsl::card_data::{CardMetadata, CardType};
use card_dsl::dsl::{Ability, ActionDesignator, Trigger, UsageLimit};

use crate::card_registry::{self, CardRegistry};
use crate::engine::dispatch::{abilities, reaction_windows};
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::{abilities_in_effect, ability_source};
use crate::state::{
    AbilitySource, CandidateSource, CardCode, GameState, InvestigatorId, ResolutionCandidate,
    Status,
};

/// Which path is asking. The kind decides which of the gate's checks apply
/// ([`applies`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InitiationKind {
    /// A forced ability at its timing point — the forced collector.
    Forced,
    /// A `[reaction]` ability on an ability source — the in-play and act/agenda
    /// reaction scans.
    Reaction,
    /// An `[action]` or `[fast]` activated ability — the activation validator.
    Activated,
    /// Playing a card from hand — the play validator, and a Fast event offered
    /// in a reaction window, which is *played* (`glossary/Fast.md`: *"A fast
    /// card does not cost an action to be played and is not played using the
    /// "Play" action."*).
    Play,
}

/// Why the gate refused — one variant per check.
///
/// Local to the gate: engine-wide rejections stay strings, and a refusal
/// renders to one (`Cow<'static, str>: From<Refusal>`) where it meets a
/// handler's `EngineOutcome::Rejected`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    /// The controller is not [`Status::Active`]; `None` when they are not in
    /// the game state at all.
    NotActive {
        /// The would-be controller.
        investigator: InvestigatorId,
        /// Their status, if they are seated.
        status: Option<Status>,
    },
    /// Nothing resolves at the candidate's address on the side of its source
    /// that is in effect, or among the grants it holds now.
    SideNotInEffect,
    /// The ability's eligibility predicate is false, or has no registered
    /// predicate to evaluate.
    NotEligible,
    /// The ability's effect has no potential to change the game state.
    NoStateChange,
    /// The ability has been initiated as many times as its printed limit allows
    /// this period.
    UsageLimitReached,
    /// A constant "cannot play" forbids the controller this card's type
    /// (Dissonant Voices 01165: *"You cannot play assets or events."*).
    PlayBanned {
        /// The would-be player.
        investigator: InvestigatorId,
        /// The forbidden card type.
        card_type: CardType,
    },
    /// The cost cannot be paid in full; carries the reason the cost check gave.
    CostUnpayable(Cow<'static, str>),
}

impl From<Refusal> for Cow<'static, str> {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::NotActive {
                investigator,
                status: Some(status),
            } => format!("{investigator:?} is not Active (status {status:?})").into(),
            Refusal::NotActive {
                investigator,
                status: None,
            } => format!("investigator {investigator:?} is not in state").into(),
            Refusal::SideNotInEffect => "the ability is not in effect on its source".into(),
            Refusal::NotEligible => "the ability's eligibility condition is not met".into(),
            Refusal::NoStateChange => {
                "the ability's effect has no potential to change the game state".into()
            }
            Refusal::UsageLimitReached => "the ability has reached its usage limit".into(),
            Refusal::PlayBanned {
                investigator,
                card_type,
            } => format!(
                "PlayCard: {investigator:?} cannot play a {card_type:?} \
                 (a constant restriction forbids it)"
            )
            .into(),
            Refusal::CostUnpayable(reason) => reason,
        }
    }
}

/// One of the gate's optional checks. Resolving the address — side in effect —
/// is not among them: every kind needs the ability before it can ask anything
/// else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    Status,
    StateChange,
    Eligibility,
    UsageLimit,
    PlayBan,
    Cost,
}

/// **The check × kind table** — which preliminary confirmations each kind gets.
///
/// - **Status.** Reaction, Activated and Play — an eliminated investigator
///   initiates nothing (`glossary/Elimination.md`: *"The only manner in which
///   eliminated investigators interact with the game is when establishing "per
///   investigator" values"*). A forced ability's collector arms decide for
///   themselves: the round-end and game-end arms reach only Active
///   investigators, while the elimination game-end collector deliberately
///   reaches the investigator being eliminated.
/// - **Potential to change the game state, and eligibility.** Every kind. A
///   forced ability gets exactly these (`glossary/Ability.md`: *"If a forced
///   ability does not have the potential to change the game state, the ability
///   does not initiate."*).
/// - **Usage limit.** Reaction and Activated — the triggered abilities a player
///   chooses to initiate. A forced ability gets only the two checks above, and
///   a played card has no ability whose limit the gate could count.
/// - **Play-ban.** Play only. A ban on playing a card type says nothing about
///   an in-play card's abilities.
/// - **Cost.** Every kind but Forced, which is not paid for.
const fn applies(check: Check, kind: InitiationKind) -> bool {
    use InitiationKind::{Activated, Forced, Play, Reaction};
    match check {
        Check::Status => matches!(kind, Reaction | Activated | Play),
        Check::StateChange | Check::Eligibility => true,
        Check::UsageLimit => matches!(kind, Reaction | Activated),
        Check::PlayBan => matches!(kind, Play),
        Check::Cost => !matches!(kind, Forced),
    }
}

/// Whether `candidate` may initiate as `kind` — Appendix I's two preliminary
/// confirmations, restricted to the checks [`applies`] gives `kind`.
///
/// **The gate resolves the ability from the candidate's address itself**,
/// through the side-in-effect / grant funnel
/// ([`abilities_in_effect::resolve`]), so a side that has turned over or a
/// grant that has lapsed is refused by the same code whether the option is
/// being offered or resolved.
///
/// Pure over `&GameState`, which is what lets a reaction window re-ask it once
/// a sibling option has resolved (#568).
///
/// The **change-state** check is [`evaluator::effect_can_change_state`],
/// conservative by construction; the **eligibility** tag refines opaque
/// `Effect::Native` effects it can't introspect (#368: Cover Up 01007's *"if
/// there are any clues on Cover Up"*, act 01109). No tag → eligible. A tag
/// with no resolvable predicate → refused, so a half-installed host never
/// surfaces a gated ability it can't evaluate.
///
/// The **cost** check asks whether each printed cost can be paid in full
/// against the controller and the source, and for a Play whether the card's
/// resource cost can. Cost *modifiers* are not modelled.
///
/// Refuses with [`Refusal::SideNotInEffect`] when no registry is installed:
/// with nothing to resolve the address against, nothing is in effect.
pub(super) fn check(
    state: &GameState,
    candidate: &ResolutionCandidate,
    kind: InitiationKind,
) -> Result<(), Refusal> {
    let reg = card_registry::current().ok_or(Refusal::SideNotInEffect)?;
    check_with(state, reg, candidate, kind)
}

/// [`check`] against an explicitly supplied registry — the gate seam's tests
/// pass a mock rather than installing one into the process-global `OnceLock`.
pub(super) fn check_with(
    state: &GameState,
    reg: &CardRegistry,
    candidate: &ResolutionCandidate,
    kind: InitiationKind,
) -> Result<(), Refusal> {
    let controller = candidate.controller;
    if applies(Check::Status, kind) {
        match state.investigators.get(&controller).map(|inv| inv.status) {
            Some(Status::Active) => {}
            status => {
                return Err(Refusal::NotActive {
                    investigator: controller,
                    status,
                })
            }
        }
    }
    let ability = abilities_in_effect::resolve_with(
        state,
        reg,
        candidate.source,
        &candidate.code,
        &candidate.address,
    )
    .ok_or(Refusal::SideNotInEffect)?;
    restrictions_met(state, reg, &ability, candidate.source, controller, kind)?;
    if applies(Check::UsageLimit, kind) && usage_exhausted(state, candidate, ability.usage_limit) {
        return Err(Refusal::UsageLimitReached);
    }
    if applies(Check::PlayBan, kind) {
        if let Some(card_type) = (reg.metadata_for)(&candidate.code).map(CardMetadata::card_type) {
            if evaluator::play_is_prohibited(state, reg, controller, card_type) {
                return Err(Refusal::PlayBanned {
                    investigator: controller,
                    card_type,
                });
            }
        }
    }
    if applies(Check::Cost, kind) {
        cost_payable(state, reg, &ability, candidate, kind).map_err(Refusal::CostUnpayable)?;
    }
    Ok(())
}

/// The change-state and eligibility checks on an already-resolved `ability`.
fn restrictions_met(
    state: &GameState,
    reg: &CardRegistry,
    ability: &Ability,
    source: CandidateSource,
    controller: InvestigatorId,
    kind: InitiationKind,
) -> Result<(), Refusal> {
    let ctx = EvalContext::for_controller_with_optional_source(controller, source.ability());
    if applies(Check::StateChange, kind)
        && !performs_an_action(ability)
        && !evaluator::effect_can_change_state(state, ctx, &ability.effect)
    {
        return Err(Refusal::NoStateChange);
    }
    if applies(Check::Eligibility, kind) {
        if let Some(tag) = ability.eligibility.as_deref() {
            if !(reg.native_eligibility_for)(tag).is_some_and(|pred| pred(state, &ctx)) {
                return Err(Refusal::NotEligible);
            }
        }
    }
    Ok(())
}

/// Whether `ability` is an activated ability whose bold action designator
/// performs an action — every designator but **Parley**, which performs nothing.
///
/// Such an ability's substance is the action, not the residual effect printed
/// beside the bold word (#805; `glossary/Ability.md`: *"Activating such an
/// ability **performs the designated action**"*), and every implemented one's
/// residual is empty, which [`evaluator::effect_can_change_state`] proves inert.
/// So the change-state question is asked of the **action** instead, by
/// `designator::can_perform` — no co-located enemy, no Fight; no revealed
/// location, no Investigate. That is a designator-target check, which ADR 0017
/// leaves with the activation path: `check_activate_ability` asks it before it
/// asks this gate. A Parley ability falls through to the residual, which is all
/// it has to change the game state with.
fn performs_an_action(ability: &Ability) -> bool {
    matches!(
        &ability.trigger,
        Trigger::Activated {
            designator: Some(designator),
            ..
        } if !matches!(designator, ActionDesignator::Parley)
    )
}

/// Whether `ability` may initiate on its change-state and eligibility checks
/// alone, for the two callers that ask about an ability they already hold
/// rather than about an address.
///
/// Transitional: `TODO(#957)` moves the hand Fast-event scan onto [`check`] as
/// [`InitiationKind::Play`], and `TODO(#960)` moves `lapse_reason` onto it;
/// this goes once both have.
pub(super) fn ability_can_initiate(
    state: &GameState,
    ability: &Ability,
    source: CandidateSource,
    controller: InvestigatorId,
) -> bool {
    card_registry::current().is_some_and(|reg| {
        restrictions_met(
            state,
            reg,
            ability,
            source,
            controller,
            InitiationKind::Reaction,
        )
        .is_ok()
    })
}

/// Whether the ability `candidate` names has used up its printed limit this
/// period. Only a printed ability on an in-play card instance carries a
/// counter: a granted ability has no printed index to key one by (#829), and a
/// location, enemy, act or agenda has no instance to hold one (#699). Neither
/// is counted out here, and the activation validator refuses either one that
/// prints a limit (ADR 0010).
fn usage_exhausted(
    state: &GameState,
    candidate: &ResolutionCandidate,
    limit: Option<UsageLimit>,
) -> bool {
    let (Some(index), Some(source)) = (
        candidate.address.printed_index(),
        candidate.source.ability(),
    ) else {
        return false;
    };
    ability_source::source_card(state, source)
        .and_then(|card| {
            card.instance()
                .map(|instance| instance.is_usage_exhausted(index, limit, state.round))
        })
        .unwrap_or(false)
}

/// Whether the cost of initiating `ability` as `kind` can be paid in full.
fn cost_payable(
    state: &GameState,
    reg: &CardRegistry,
    ability: &Ability,
    candidate: &ResolutionCandidate,
    kind: InitiationKind,
) -> Result<(), Cow<'static, str>> {
    if kind == InitiationKind::Play {
        return play_cost_payable(state, reg, candidate.controller, &candidate.code);
    }
    if ability.costs.is_empty() {
        return Ok(());
    }
    let Some(inv) = state.investigators.get(&candidate.controller) else {
        return Err(format!(
            "investigator {ctl:?} is not in state",
            ctl = candidate.controller
        )
        .into());
    };
    let source = candidate
        .source
        .ability()
        .and_then(|source| ability_source::source_card(state, source));
    let exhausted = source.is_some_and(|card| card.exhausted());
    let uses = source.map(|card| card.uses()).unwrap_or_default();
    for cost in &ability.costs {
        abilities::check_cost_payable(cost, inv, exhausted, &uses)?;
    }
    Ok(())
}

/// Whether `investigator` can pay `code`'s printed resource cost — the body of
/// `reaction_windows::check_play_resource_cost_payable`, which documents the
/// three shapes a printed cost takes. `Ok` for a card with no metadata.
pub(super) fn play_cost_payable(
    state: &GameState,
    reg: &CardRegistry,
    investigator: InvestigatorId,
    code: &CardCode,
) -> Result<(), Cow<'static, str>> {
    let Some(meta) = (reg.metadata_for)(code) else {
        return Ok(());
    };
    let resources = state
        .investigators
        .get(&investigator)
        .map_or(0, |inv| inv.resources);
    let cost = reaction_windows::payable_play_cost(meta.play_cost(), code)?;
    if resources < cost {
        return Err(format!(
            "PlayCard: playing {code} costs {cost} resource(s); \
             {investigator:?} has {resources}"
        )
        .into());
    }
    Ok(())
}

/// Record one initiation of the ability `candidate` names against its
/// `usage_limit` — Appendix I step 3, *"The card commences being played, or the
/// effects of the ability attempt to initiate."* Called before the effect is
/// pushed, so a use whose effects are then cancelled still counts: *"If the
/// effects of a card or ability with a limit or maximum are canceled, it is
/// still counted against the limit/maximum, because the ability has been
/// initiated."* (`data/rules-reference/rules/glossary/Limits_and_Maximums.md`).
///
/// `None` for `usage_limit` (no printed *"Limit X per \[period\]"*) records
/// nothing.
///
/// The counter is `CardInPlay::ability_usage`, per instance and keyed by
/// printed index, so this resolves the instance **wherever it sits on the
/// board** — the same walk `usage_exhausted` reads through
/// (`ability_source::source_card`). That covers the controller's investigator
/// card (Roland Banks 01001's seated `[reaction]`), cards in play and threat
/// area, and the sources an activation reaches without controlling them: a
/// co-located threat area, a location's attachments (#708). A granted ability
/// has no printed index, so nothing is recorded for one (#829).
///
/// **A limit on a source with no card instance is refused before it can reach
/// here.** A location, enemy, act or agenda has nowhere to record a use, so
/// `reject_untrackable_usage_limit` rejects the activation-side case at
/// validation, no corpus card prints such a forced or reaction ability, and one
/// reaching here is an invariant violation. #699 builds the state-level counter
/// the Dunwich locations need, and lifts both.
pub(super) fn record_initiation(
    state: &mut GameState,
    candidate: &ResolutionCandidate,
    usage_limit: Option<UsageLimit>,
) {
    if usage_limit.is_none() {
        return;
    }
    let current_round = state.round;
    match candidate.source {
        CandidateSource::Ability(AbilitySource::InPlay(instance_id)) => {
            let card =
                ability_source::instance_in_play_mut(state, instance_id).unwrap_or_else(|| {
                    unreachable!(
                        "record_initiation: instance {instance_id:?} vanished from play while \
                         its ability was initiating; state-corruption invariant violation \
                         (candidate {candidate:?})"
                    )
                });
            if let Some(index) = candidate.address.printed_index() {
                card.bump_ability_usage(index, current_round);
            }
        }
        // Listed kind by kind rather than wildcarded: a sixth `AbilitySource`
        // kind *that carries an instance* must break this build rather than
        // reach a panic at runtime.
        CandidateSource::Ability(
            AbilitySource::Location(_)
            | AbilitySource::Enemy(_)
            | AbilitySource::Act
            | AbilitySource::Agenda,
        )
        | CandidateSource::Hand => {
            unreachable!(
                "record_initiation: a usage-limited candidate must be an in-play instance \
                 (a hand candidate, and an ability source with no card instance behind it — a \
                 location, an enemy, the act, the agenda — have nowhere to record uses); \
                 candidate {candidate:?}"
            )
        }
    }
}

#[cfg(test)]
mod tests;
