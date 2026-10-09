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
//! [`record_initiation`] counts a use at step 3. [`initiate`] is the one path
//! that *fires* a triggered ability — forced or reaction, alone or in a run —
//! and [`push_bound_effect`] is the tail it shares with a Fast event played from
//! a reaction window. Each path keeps its own *when*
//! question — timing cells, `trigger_matches`, the scans' scoping, the
//! turn/Fast matrix, slots and the action economy — and asks this module only
//! *whether*. The checks each [`InitiationKind`] gets are one table,
//! [`applies`].

use std::borrow::Cow;

use card_dsl::card_data::{CardMetadata, CardType};
use card_dsl::dsl::{Ability, ActionDesignator, Effect, Trigger, UsageLimit};

use crate::card_registry::{self, CardRegistry};
use crate::engine::dispatch::abilities;
use crate::engine::dispatch::emit::TimingEvent;
use crate::engine::evaluator::{self, EvalContext};
use crate::engine::{abilities_in_effect, ability_source, Cx};
use crate::state::{
    AbilitySource, CandidateSource, CardCode, DamageSource, GameState, Investigator,
    InvestigatorId, ResolutionCandidate, Status,
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
/// - **Potential to change the game state, and eligibility.** Every kind
///   (`glossary/Ability.md`: *"If a forced ability does not have the potential
///   to change the game state, the ability does not initiate."*).
/// - **Usage limit.** Forced, Reaction and Activated — every ability that
///   initiates. `glossary/Limits_and_Maximums.md`: *"Each instance of an
///   ability with such a limit may be initiated X times during the designated
///   period."* It makes no exception for forced abilities, and
///   `glossary/Ability.md` says they initiate too: *"Forced abilities initiate
///   and interact with the game state automatically at a specified timing
///   point."* A played card has no ability whose limit the gate could count.
/// - **Play-ban.** Play only. A ban on playing a card type says nothing about
///   an in-play card's abilities.
/// - **Cost.** Every kind but Forced, which is not paid for.
const fn applies(check: Check, kind: InitiationKind) -> bool {
    use InitiationKind::{Activated, Forced, Play, Reaction};
    match check {
        Check::Status => matches!(kind, Reaction | Activated | Play),
        Check::StateChange | Check::Eligibility => true,
        Check::UsageLimit => matches!(kind, Forced | Reaction | Activated),
        Check::PlayBan => matches!(kind, Play),
        Check::Cost => !matches!(kind, Forced),
    }
}

// Every kind the cost check applies to is one the status check applies to, so
// the cost check always reads a seated investigator ([`confirm`]). Asserted at
// compile time: a table edit that breaks it fails the build rather than
// reaching `confirm`'s `unreachable!`.
const _: () = {
    let kinds = [
        InitiationKind::Forced,
        InitiationKind::Reaction,
        InitiationKind::Activated,
        InitiationKind::Play,
    ];
    let mut i = 0;
    while i < kinds.len() {
        assert!(!applies(Check::Cost, kinds[i]) || applies(Check::Status, kinds[i]));
        i += 1;
    }
};

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
    confirm(
        state,
        reg,
        candidate.controller,
        &candidate.code,
        kind,
        || {
            let ability = abilities_in_effect::resolve_with(
                state,
                reg,
                candidate.source,
                &candidate.code,
                &candidate.address,
            )
            .ok_or(Refusal::SideNotInEffect)?;
            restrictions_met(
                state,
                reg,
                &ability,
                candidate.source,
                candidate.controller,
                kind,
            )?;
            if applies(Check::UsageLimit, kind)
                && usage_exhausted(state, candidate, ability.usage_limit)
            {
                return Err(Refusal::UsageLimitReached);
            }
            Ok(ability)
        },
        |inv, ability| {
            if kind == InitiationKind::Play {
                play_cost_payable(reg, inv, &candidate.code)
            } else {
                ability_cost_payable(state, inv, ability, candidate.source)
            }
        },
    )
}

/// Whether `controller` may play `code` from hand — [`check`] as
/// [`InitiationKind::Play`] for the play validator, which names a card rather
/// than one of its abilities.
///
/// A Fast event offered in a reaction window names the `[reaction]` ability
/// whose timing it plays at, so it asks [`check`] with that address. A card
/// played from the turn menu or a player window has no such ability: what it
/// initiates is the card itself, so there is no address to resolve. The
/// change-state and eligibility checks read the effect the play resolves
/// instead — an event's `OnPlay` abilities, of which at least one must pass
/// (Working a Hunch 01037 at a 0-clue location is unplayable). An asset
/// changes the game state by entering play, so it passes them unasked. Every
/// other Play check — status, play-ban, cost — is the same code [`check`]
/// runs.
///
/// Refuses with [`Refusal::SideNotInEffect`] when no registry is installed or
/// it does not know `code`, as [`check`] does for an address it cannot
/// resolve.
pub(super) fn check_play(
    state: &GameState,
    controller: InvestigatorId,
    code: &CardCode,
) -> Result<(), Refusal> {
    let reg = card_registry::current().ok_or(Refusal::SideNotInEffect)?;
    check_play_with(state, reg, controller, code)
}

/// [`check_play`] against an explicitly supplied registry, for the gate seam's
/// tests.
pub(super) fn check_play_with(
    state: &GameState,
    reg: &CardRegistry,
    controller: InvestigatorId,
    code: &CardCode,
) -> Result<(), Refusal> {
    let kind = InitiationKind::Play;
    confirm(
        state,
        reg,
        controller,
        code,
        kind,
        || {
            let card_type = (reg.metadata_for)(code)
                .map(CardMetadata::card_type)
                .ok_or(Refusal::SideNotInEffect)?;
            if card_type != CardType::Event {
                return Ok(());
            }
            // The first refusal when no `OnPlay` effect passes; `NoStateChange`
            // when the event has none at all, since nothing would resolve.
            let mut refusal = None;
            let playable = (reg.abilities_for)(code)
                .unwrap_or_default()
                .iter()
                .filter(|ability| matches!(ability.trigger, Trigger::OnPlay))
                .any(|ability| {
                    restrictions_met(state, reg, ability, CandidateSource::Hand, controller, kind)
                        .map_err(|r| refusal.get_or_insert(r))
                        .is_ok()
                });
            if playable {
                Ok(())
            } else {
                Err(refusal.unwrap_or(Refusal::NoStateChange))
            }
        },
        |inv, ()| play_cost_payable(reg, inv, code),
    )
}

/// **The chain every check runs**, in Appendix I's order: status, then what is
/// being initiated — `subject`, which resolves it and runs its change-state,
/// eligibility and usage-limit checks — then the play-ban, then the cost. Each
/// check runs only where [`applies`] gives it `kind`; `subject` applies the
/// table to its own checks.
///
/// `cost` is handed the seated controller and whatever `subject` resolved. It
/// is only called where the cost check applies, and every such kind gets the
/// status check too (asserted at compile time beside [`applies`]), so the
/// controller is always seated by then.
fn confirm<'s, T>(
    state: &'s GameState,
    reg: &CardRegistry,
    controller: InvestigatorId,
    code: &CardCode,
    kind: InitiationKind,
    subject: impl FnOnce() -> Result<T, Refusal>,
    cost: impl FnOnce(&'s Investigator, &T) -> Result<(), Cow<'static, str>>,
) -> Result<(), Refusal> {
    let seated = if applies(Check::Status, kind) {
        let inv = state.investigators.get(&controller);
        match inv.map(|inv| inv.status) {
            Some(Status::Active) => inv,
            status => {
                return Err(Refusal::NotActive {
                    investigator: controller,
                    status,
                })
            }
        }
    } else {
        None
    };
    let subject = subject()?;
    play_not_banned(state, reg, controller, code, kind)?;
    if applies(Check::Cost, kind) {
        let inv = seated.unwrap_or_else(|| {
            unreachable!(
                "initiation: the cost check applies to {kind:?}, so the status check does too \
                 and has seated {controller:?}"
            )
        });
        cost(inv, &subject).map_err(Refusal::CostUnpayable)?;
    }
    Ok(())
}

/// The play-ban check: no constant "cannot play" forbids `controller` the
/// card type of `code`, when it applies to `kind`.
fn play_not_banned(
    state: &GameState,
    reg: &CardRegistry,
    controller: InvestigatorId,
    code: &CardCode,
    kind: InitiationKind,
) -> Result<(), Refusal> {
    if !applies(Check::PlayBan, kind) {
        return Ok(());
    }
    match (reg.metadata_for)(code).map(CardMetadata::card_type) {
        Some(card_type) if evaluator::play_is_prohibited(state, reg, controller, card_type) => {
            Err(Refusal::PlayBanned {
                investigator: controller,
                card_type,
            })
        }
        _ => Ok(()),
    }
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

/// Whether `inv` can pay every printed cost of initiating `ability` from
/// `source` in full.
fn ability_cost_payable(
    state: &GameState,
    inv: &Investigator,
    ability: &Ability,
    source: CandidateSource,
) -> Result<(), Cow<'static, str>> {
    if ability.costs.is_empty() {
        return Ok(());
    }
    let source = source
        .ability()
        .and_then(|source| ability_source::source_card(state, source));
    let exhausted = source.is_some_and(|card| card.exhausted());
    let uses = source.map(|card| card.uses()).unwrap_or_default();
    for cost in &ability.costs {
        abilities::check_cost_payable(cost, inv, exhausted, &uses)?;
    }
    Ok(())
}

/// Playing a card is paying its resource cost in full (RR p.22, Initiation
/// Sequence — the cost must be established as payable before initiation, and is
/// then paid before attacks of opportunity resolve). Returns the reject reason
/// when `inv` cannot pay `code`'s printed cost. A 0-cost card is always
/// affordable.
///
/// The two costs that are not a number reject for **different reasons**, and
/// [`CardMetadata::play_cost`](card_dsl::card_data::CardMetadata::play_cost) —
/// which owns the description of how each one is encoded — is what tells them
/// apart.
///
/// - **`None` — a `"–"` cost, which includes every permanent.** Rejected
///   **permanently**, not pending a model: per the official FAQ, *"Cards with
///   a cost of '–' have no cost that can be paid, and therefore cannot be
///   played. … (Cards that put it directly into play bypassing its cost would
///   be able to put it into play, however.)"*
///   (`data/official-faq/Frequently_Asked_Questions.md`.) This is live in the
///   corpus — The Necronomicon 01009 and every Dunwich permanent — and the
///   rejection is the final behaviour. Putting such a card into play without
///   playing it is a different path and does not come through here.
/// - **`Some(n)` with `n < 0` — an X cost.** Genuinely **not yet modeled**
///   (deferral split from #501): X needs a player-chosen amount the play path
///   has no channel for. Rejected loudly, because the alternative is worse
///   than a reject — `u8::try_from(-2).unwrap_or(0)` would make the card
///   *free*, both here and at `pay_play_cost`. Jenny's Twin .45s 02010 is
///   in the compiled corpus, so this arm is reachable the moment an X-cost
///   card gets an implementation.
///
/// `Ok` for a card with no metadata — the registry-free validation paths
/// the engine's own unit tests exercise.
fn play_cost_payable(
    reg: &CardRegistry,
    inv: &Investigator,
    code: &CardCode,
) -> Result<(), Cow<'static, str>> {
    let Some(meta) = (reg.metadata_for)(code) else {
        return Ok(());
    };
    let cost = payable_play_cost(meta.play_cost(), code)?;
    if inv.resources < cost {
        return Err(format!(
            "PlayCard: playing {code} costs {cost} resource(s); {investigator:?} has {resources}",
            investigator = inv.id,
            resources = inv.resources,
        )
        .into());
    }
    Ok(())
}

/// Classify a printed play cost into a payable number of resources, or the
/// reason it has none. The three shapes and why they differ are spelled out on
/// [`play_cost_payable`]; this is the arm split on its own so it can be tested
/// without a registry.
fn payable_play_cost(play_cost: Option<i8>, code: &CardCode) -> Result<u8, Cow<'static, str>> {
    match play_cost {
        // A negative cost is ArkhamDB's X sentinel, never a real price;
        // `u8::try_from` would silently make it free, so it rejects here.
        Some(cost) => u8::try_from(cost).map_err(|_| {
            Cow::from(format!(
                "PlayCard: {code} has an X cost, which is not yet modeled \
                 (TODO(#577): X needs a player-chosen amount)."
            ))
        }),
        None => Err(format!(
            "PlayCard: {code} has a printed cost of \"–\", so it has no cost \
             that can be paid and cannot be played."
        )
        .into()),
    }
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

/// **Fire** the triggered ability `candidate` names, at the timing point
/// `event` — the one firing path for every forced and reaction ability: a lone
/// forced hit, an ability the lead picks from an ordered forced run, and a
/// reaction a player picks from a window. Four steps, in Appendix I's order:
///
/// 1. resolve the ability at its address through the side-in-effect / grant
///    funnel ([`abilities_in_effect::resolve`]);
/// 2. bind what `event` supplies ([`push_bound_effect`]);
/// 3. record the initiation ([`record_initiation`], step 3 — before the effect
///    is pushed, so a use whose effects are cancelled still counts);
/// 4. push the effect for the `drive` loop.
///
/// Step 3 applies to forced abilities exactly as to reactions.
/// `glossary/Limits_and_Maximums.md`: *"Each instance of an ability with such
/// a limit may be initiated X times during the designated period."* — with no
/// exception for forced abilities, which `glossary/Ability.md` says *"initiate
/// and interact with the game state automatically at a specified timing
/// point."*
///
/// Returns the pushed effect, so the lone forced path can decide whether it
/// warrants a #466 acknowledge. Refuses with [`Refusal::SideNotInEffect`],
/// having changed nothing, when no ability resolves at the address.
///
/// **Not the activation path.** An activated ability runs the effect it
/// snapshotted before paying its costs (a cost may discard the source), has no
/// timing event to bind from, and records its own use. A Fast event from hand
/// is not fired here either: it is *played*, and shares only the bind-and-push
/// tail.
pub(super) fn initiate(
    cx: &mut Cx,
    candidate: &ResolutionCandidate,
    event: &TimingEvent,
) -> Result<Effect, Refusal> {
    let ability = abilities_in_effect::resolve(
        cx.state,
        candidate.source,
        &candidate.code,
        &candidate.address,
    )
    .ok_or(Refusal::SideNotInEffect)?;
    // The source rides along as an `AbilitySource`, so an effect that refers to
    // itself (`DiscardSelf`) finds the firing card, and an effect-internal
    // `ChooseOne` anchors its options to it — including the act's and agenda's
    // reverses, which have no card instance (#555).
    let ctx = EvalContext::for_controller_with_optional_source(
        candidate.controller,
        candidate.source.ability(),
    );
    record_initiation(cx.state, candidate, ability.usage_limit);
    push_bound_effect(cx, &ability.effect, ctx, event);
    Ok(ability.effect)
}

/// Bind what `event` supplies into `ctx`, then push `effect` for the `drive`
/// loop — the tail [`initiate`] shares with a Fast event played from a
/// reaction window, so an effect naming *"that enemy"* or *"that many"* gets the
/// same binding however it was reached.
///
/// - An enemy attack's damage assignment binds the **attacking enemy** — Guard
///   Dog 01021's retaliate names it.
/// - A clue discovery binds the **capped** count. `discover_clue` caps at the
///   location's clues before emitting (#471), so *"that many"* (Cover Up 01007)
///   is what would actually have been discovered, not what was requested.
pub(super) fn push_bound_effect(
    cx: &mut Cx,
    effect: &Effect,
    mut ctx: EvalContext,
    event: &TimingEvent,
) {
    match event {
        TimingEvent::DamageAssigned {
            source: DamageSource::EnemyAttack { enemy },
            ..
        } => ctx.set_attacking_enemy(*enemy),
        TimingEvent::DiscoverClues { count, .. } => ctx.set_clue_discovery_count(*count),
        _ => {}
    }
    evaluator::push_effect(cx, effect, ctx);
}

#[cfg(test)]
mod tests;
