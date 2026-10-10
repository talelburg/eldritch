//! Whether a card may be played from hand ([`check_play_card`]) or an
//! activated ability activated ([`check_activate_ability`]) right now: the pure
//! validators the `play_card` / `activate_ability` handlers, the open-turn menu
//! and the Fast-window enumeration all ask, so none of them can offer what the
//! others refuse.
//!
//! Each path keeps its own timing here — the play timing matrix and the
//! activation's action points (ADR 0017) — and asks the initiation gate
//! *whether*. What taking the action costs is the take-an-action step's
//! answer: a non-fast play and an action-cost ability ask [`take::check`], so
//! the surcharge a validator would refuse on is the one the step charges.
//! A fast play or fast ability takes no action and never asks it.

use std::borrow::Cow;

use card_dsl::card_data::{CardMetadata, CardType};
use card_dsl::dsl::{
    Ability, ActionDesignator, Cost, Effect, EnemyTarget, Trigger, TriggerKind, UsageLimit,
};

use crate::card_registry;
use crate::engine::dispatch::abilities::{self, ActivatedAbility};
use crate::engine::dispatch::actions::take::{self, ActionDescription};
use crate::engine::dispatch::initiation::{self, InitiationKind, Refusal};
use crate::engine::dispatch::{cards, combat, slots};
use crate::engine::outcome::EngineOutcome;
use crate::engine::{ability_source, designator};
use crate::state::{
    AbilityAddress, AbilitySource, CandidateSource, CardCode, GameState, InvestigatorId, Phase,
    ResolutionCandidate,
};

/// Validated payload returned by [`check_play_card`] on success.
/// Carries the data `play_card`'s mutation step needs without
/// re-running the validation.
///
/// `is_fast` is consumed by
/// [`any_fast_play_eligible`](super::reaction_windows::any_fast_play_eligible);
/// `abilities` is kept for future consumers (e.g. reaction-window dispatch).
///
/// The card's destination is deliberately **not** here: commencing a play is
/// destination-agnostic (asset and event alike leave hand at RR Appendix I step
/// 3), and the disposal that needs it re-derives it from the code at step 4
/// (#604).
///
/// `#[allow(dead_code)]` covers `abilities` (not yet read outside validation)
/// and suppresses the rustc `dead_code` lint on struct fields that are only read
/// by a `pub(super)` function not yet wired up.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct PlayCheckResult {
    pub abilities: Vec<Ability>,
    pub is_fast: bool,
    pub card_type: CardType,
}

/// Validated payload returned by [`check_activate_ability`] on success.
/// Carries the data `activate_ability`'s mutation step needs without
/// re-running the validation.
#[derive(Debug)]
#[allow(dead_code)] // Fields consumed by any_fast_play_eligible in T05.
pub(crate) struct ActivateCheckResult {
    /// The activation as the initiation gate checked it — the source card's
    /// code, the activating investigator, the ability's address and its source.
    /// What `initiation::record_initiation` counts the use against, so the
    /// handler records the very candidate the gate approved.
    pub candidate: ResolutionCandidate,
    /// The action points the ability prints (its `Trigger::Activated` cost),
    /// without any surcharge. Taking the activation as an action adds the
    /// `ExtraActionCost` surcharge on the class its designator names (#754),
    /// pays the total and marks the surcharge sources.
    pub action_cost: u8,
    /// The bold action designator the ability prints, if any — what the
    /// attack-of-opportunity exemption reads (#696) and what names the action
    /// class the surcharge keys on (#754).
    pub designator: Option<ActionDesignator>,
    /// Payment costs (beyond the action cost).
    pub costs: Vec<Cost>,
    /// The effect to dispatch after paying costs.
    pub effect: Effect,
    /// The *"Limit X per \[period\]"* cap, if the ability prints one — what
    /// `initiation::record_initiation` counts the activation against.
    pub usage_limit: Option<UsageLimit>,
    /// Whether the source card was exhausted at validation time —
    /// load-bearing for activated abilities whose payment includes
    /// `Cost::Exhaust`.
    pub source_exhausted: bool,
}

/// Gates RR p.19 slot capacity: Assets only; the only hard slot reject — a merely-full
/// slot is not rejected here, make-room at enter-play handles it.
fn check_play_slot_satisfiable(
    card_type: CardType,
    code: &CardCode,
) -> Result<(), Cow<'static, str>> {
    if card_type != CardType::Asset {
        return Ok(());
    }
    if let Some(slot) = slots::unsatisfiable_slot(code) {
        return Err(format!(
            "PlayCard: {code} needs more {slot:?} slots than the investigator has \
             (slot capacity exceeded; RR p.19)."
        )
        .into());
    }
    Ok(())
}

/// Pure-validation peer to [`play_card`](cards::play_card). Returns `Ok` if the named
/// card is currently playable by `investigator`, `Err(reason)` if
/// not.
///
/// *Whether* the card may be played is the initiation gate's answer, asked as
/// a play ([`initiation::check_play`]); this validator owns only the *when* and
/// the rest of what a play from the turn menu or a player window needs — the
/// hand index, reaction events, slots and the turn/Fast timing matrix (ADR
/// 0017). A non-fast play's action point is the step's answer
/// ([`take::check`]).
///
/// Used by [`play_card`](cards::play_card) (which then runs the mutation block on the
/// `Ok` payload) and by `any_fast_play_eligible` (which only
/// inspects `Ok` vs `Err`).
pub(crate) fn check_play_card(
    state: &GameState,
    investigator: InvestigatorId,
    hand_index: u8,
) -> Result<PlayCheckResult, Cow<'static, str>> {
    let Some(inv) = state.investigators.get(&investigator) else {
        return Err(format!("PlayCard: investigator {investigator:?} is not in state").into());
    };
    let idx = usize::from(hand_index);
    if idx >= inv.hand.len() {
        return Err(format!(
            "PlayCard: hand_index {hand_index} out of bounds (hand size {})",
            inv.hand.len(),
        )
        .into());
    }
    let code: CardCode = inv.hand[idx].clone();
    // Resolve card type and abilities (also yields is_fast + card_type) before
    // applying the phase/active-investigator gate so the gate can branch on
    // is_fast AND card_type per the Rules Reference (p. 11).
    // Invariant: `resolve_play_target` currently returns only `Ok(...)` (success)
    // or `Err(EngineOutcome::Rejected { ... })` (validation failure). If a future
    // PR extends it to return `AwaitingInput` (e.g. for a card requiring in-
    // validation target selection), this `unreachable!()` will panic; the
    // validator's caller chain in `play_card` would need to be redesigned to
    // thread the `AwaitingInput` outcome back through `check_play_card`'s
    // `Result` shape. Pinning the invariant loudly here is intentional —
    // silent `AwaitingInput` propagation through a `Result<_, Cow>` would
    // produce wrong gameplay.
    // The destination is re-derived from the code at disposal time
    // (`dispose_play_from_hand`), not carried through validation — commencing a
    // play is destination-agnostic (#604).
    let (_destination, abilities, is_fast, card_type) = match cards::resolve_play_target(&code) {
        Ok(v) => v,
        Err(EngineOutcome::Rejected { reason }) => return Err(reason),
        Err(other) => {
            unreachable!("resolve_play_target returned non-Rejected outcome: {other:?}")
        }
    };
    // Reaction-event gate (Axis C, #335 / #304): a Fast event whose play
    // instruction is a triggering condition is modeled as a `TriggerKind::Reaction`
    // `OnEvent` ability (e.g. Evidence! 01022's "Play after you defeat an enemy").
    // RR p.11: such an event "may be played any time its play instructions
    // specify" — i.e. ONLY in its matching reaction window, where Axis C offers
    // it as a `PickSingle` option (the window path runs `play_fast_event`,
    // bypassing this gate). It is never a free-timing standalone play, so reject
    // it from the `PlayCard` action — otherwise `play_card` would run only its
    // (absent) `OnPlay` abilities and silently discard it for no effect.
    //
    // Gate only on a **Reaction** `OnEvent`: an event that plays normally (an
    // `OnPlay` effect) but carries a **Forced** `OnEvent` for its *in-play*
    // form is not a reaction event (Barricade 01038 attaches on play, then its
    // attachment's Forced discards it on leave). Such an event is played as a
    // standard action.
    if card_type == CardType::Event
        && abilities.iter().any(|a| {
            matches!(
                a.trigger,
                Trigger::OnEvent {
                    kind: TriggerKind::Reaction,
                    ..
                }
            )
        })
    {
        return Err(format!(
            "PlayCard: {code} is a reaction event — it may only be played in response \
             to its triggering condition (its reaction window), not as a standalone \
             action (RR p.11)."
        )
        .into());
    }
    // The initiation gate, as a play (ADR 0017): the investigator is Active, an
    // event's effect can change the game state (#495), no "cannot play" forbids
    // the card's type (Dissonant Voices 01165, #852), and its resource cost can
    // be paid — Fast only skips the *action* cost (#501). Asked here rather than
    // in the `play_card` handler so every consumer of this validator — the
    // open-turn menu, `enumerate_fast_plays` and the handler — agrees, and a card
    // that can't be played is never *offered*.
    initiation::check_play(state, investigator, &code)?;
    // RR p.19 slots (#498): reject only when the card needs more of a slot type
    // than the investigator has capacity for — unsatisfiable even after discarding
    // every occupying asset. A merely-full slot is NOT rejected here; the play
    // proceeds and discards occupiers to make room at enter-play time. Unreachable
    // in the current corpus (max need is Hand×2 = cap 2); no silent no-op.
    check_play_slot_satisfiable(card_type, &code)?;
    // Timing gate — see play_card doc-comment "# Timing gate" section.
    let active_during_investigation =
        state.phase == Phase::Investigation && state.active_investigator == Some(investigator);
    let owner_is_active = state.active_investigator == Some(investigator);
    let permissive_window = state
        .top_window()
        .is_some_and(|w| w.permits_fast(investigator));
    // "Play only during your turn" (Mind over Matter 01036, Working a Hunch
    // 01037, …): a Fast card with this clause is restricted to the active
    // investigator's Investigation turn — never an out-of-turn permissive Fast
    // window (the Mythos `MythosAfterDraws` window). FAQ: "'your turn' is within
    // the Investigation phase."
    //
    // The companion clause bounds the *other* end of the turn, and the gate
    // above is deliberately inclusive of it: the end of your turn has not left
    // your turn. `data/official-faq/Frequently_Asked_Questions.md`, on Agatha
    // Crane's end-of-turn reaction — *"You resolve Agatha Crane's ability at
    // the end of your turn, which is still during your turn. So you could play
    // an event such as Cryptic Research ([core] 43) with the text 'Fast. Play
    // only during your turn.'"* The engine already agrees by construction:
    // `resume_end_turn` clears `active_investigator` only *after* the
    // `EndOfTurn` forced/reaction run has resolved (`phases.rs`), so a Fast
    // play made from inside that run still sees itself as the active
    // investigator and passes this gate.
    let only_during_turn = card_registry::current()
        .and_then(|reg| (reg.metadata_for)(&code))
        .is_some_and(CardMetadata::play_only_during_turn);
    // Non-asset/non-event card types are filtered out by
    // `resolve_play_target` above, so `card_type` here is always one of
    // `Asset` or `Event`. The non-Fast arm collapses both into the
    // strict gate; the Fast arms split because Rules Reference p. 11
    // gives events and assets different scopes (any vs owner-only).
    let allowed = if is_fast {
        match card_type {
            CardType::Event => {
                if only_during_turn {
                    active_during_investigation
                } else {
                    active_during_investigation || permissive_window
                }
            }
            CardType::Asset => {
                if only_during_turn {
                    active_during_investigation
                } else {
                    active_during_investigation || (owner_is_active && permissive_window)
                }
            }
            // Unreachable: `resolve_play_target` rejects every other
            // `CardType` before we get here. Fall back to the strict
            // gate so a future relaxation of `resolve_play_target` does
            // not silently over-permit anything.
            _ => active_during_investigation,
        }
    } else {
        active_during_investigation
    };
    if !allowed {
        return Err(format!(
            "PlayCard: card not playable in this timing window. \
             Rules Reference p. 11: non-Fast cards require Investigation + active \
             investigator; Fast events require active investigator or a window whose \
             fast_actors permits the actor; Fast assets additionally require the OWNER \
             (active investigator) to act. \
             Got is_fast={is_fast}, card_type={card_type:?}, phase={phase:?}, \
             active={active:?}, actor={investigator:?}, owner_is_active={owner_is_active}, \
             permissive_window={permissive_window}.",
            phase = state.phase,
            active = state.active_investigator,
        )
        .into());
    }
    // Playing a card is an action (RR p.5), so a non-fast play is taken through
    // the step, which says whether it can be afforded (validate-first;
    // `play_card` takes it). Fast plays are not actions and never ask (#378).
    if !is_fast {
        take::check(state, investigator, &ActionDescription::play(code.clone()))
            .map_err(|why| Cow::from(format!("PlayCard: {why}")))?;
    }
    Ok(PlayCheckResult {
        abilities,
        is_fast,
        card_type,
    })
}

/// Reject an activation that cannot get what it needs, at the check layer —
/// **before any cost is paid**, so the rejection is honest for
/// `any_fast_play_eligible` and the evaluator can treat a missing target as an
/// invariant violation.
///
/// Two halves, asked in the order the ability declares them:
///
/// - **The designated action** — delegated whole to
///   [`can_perform`](crate::engine::designator::can_perform), the single
///   predicate the basic-action handlers and the turn-menu enumerator also
///   read (#805). A **Fight** needs ≥1 enemy at your location (0 = no target,
///   rejected here; 2+ suspends to a `PickSingle` in the evaluator); an
///   **Investigate** needs a revealed location, so Flashlight 01087 cannot
///   spend its supply with nothing to investigate.
/// - **`DealDamageToEnemy`:** needs ≥1 enemy in the chosen scope (e.g. "at your
///   location"). ≥1 proceeds — 2+ suspends via the `Choose` resolver — so only
///   the empty case rejects here; this is why the effect is typed, not `Native`
///   (Beat Cop can't pay its discard-self cost for no legal target). `amount` is
///   not consulted (a degenerate `amount: 0` ability — none in scope — would
///   still require a target here even though its handler is a no-op).
fn check_activation_target_available(
    state: &GameState,
    investigator: InvestigatorId,
    designator: Option<&ActionDesignator>,
    effect: &Effect,
) -> Result<(), Cow<'static, str>> {
    if let Some(designator) = designator {
        designator::can_perform(state, investigator, designator)
            .map_err(|why| Cow::from(format!("ActivateAbility: {why}")))?;
    }
    if let Effect::DealDamageToEnemy {
        target: EnemyTarget::Chosen(choose),
        ..
    } = effect
    {
        if combat::enemies_in_scope(state, investigator, choose.scope).is_empty() {
            return Err(
                "ActivateAbility: a 'deal damage to an enemy at your location' ability \
                 needs at least one enemy at your location"
                    .into(),
            );
        }
    }
    Ok(())
}

/// Render the initiation gate's [`Refusal`] in the activation validator's
/// voice, so its rejection reasons read as they did before the gate (#958).
///
/// The cost check already speaks it (`abilities::check_cost_payable`), and the
/// change-state refusal keeps its #639 wording, which cites the rule it
/// enforces — `glossary/Ability.md`, "Triggered Abilities": *"A triggered
/// ability can only be initiated if its effect has the potential to change the
/// game state, and its cost (if any) has the potential to be paid in full,
/// taking active cost modifiers into account."* Every other refusal takes the
/// validator's `ActivateAbility:` prefix.
fn activation_refusal(refusal: Refusal, code: &CardCode) -> Cow<'static, str> {
    match refusal {
        Refusal::CostUnpayable(reason) => reason,
        Refusal::NoStateChange => format!(
            "ActivateAbility: {code}'s effect cannot change the game state right now, so the \
             ability cannot be initiated (RR \"Ability\"/\"Costs\")."
        )
        .into(),
        other => format!("ActivateAbility: {}", Cow::from(other)).into(),
    }
}

/// Reject an ability mixing [`Cost::DiscardSelf`](card_dsl::dsl::Cost::DiscardSelf)
/// with another source-referencing cost: `DiscardSelf` removes the source, so it
/// must be the sole such cost (Beat Cop / Knife list only it). Deliberately
/// unlifted until a card needs the combo — no tracking issue on purpose (YAGNI);
/// whoever hits this rejection files one.
fn reject_incompatible_costs(costs: &[Cost]) -> Result<(), Cow<'static, str>> {
    if costs.iter().any(|c| matches!(c, Cost::DiscardSelf))
        && costs
            .iter()
            .any(|c| matches!(c, Cost::Exhaust | Cost::SpendUses { .. }))
    {
        return Err(
            "ActivateAbility: Cost::DiscardSelf cannot combine with Exhaust/SpendUses on the \
             same ability (it removes the source); lift if a card ever needs the combo"
                .into(),
        );
    }
    Ok(())
}

/// Reject an ability whose *"Limit X per \[period\]"* sits on a source with no
/// card instance to record the use against — a location, an enemy, the act or
/// the agenda.
///
/// Usage state is `CardInPlay::ability_usage`, a per-instance map, and a
/// location has no instance (`initiation::record_initiation`'s `unreachable!`
/// says so for the reaction path). Making these sources activatable is what first puts that
/// branch behind player input, and **a panic reachable from player input must
/// not ship** — so the limit is refused, loudly, rather than silently ignored
/// or crashed into.
///
/// No Core or Dunwich card in the corpus reaches this: the Parlor 01115's
/// Resign is unlimited. Dunwich prints two that will — Base of the Hill 02282
/// (*"\[action\]: **Investigate.** … (Limit once per round.)"*) and Ten-Acre
/// Meadow 02246 (*"(Group limit once per game)"*) — and **#699** builds the
/// capability they need. `glossary/Limits_and_Maximums.md` makes the key scoped
/// rather than global there, verbatim: *"Unless stated otherwise, limits are
/// player specific"*, while a group limit *"applies to the entire group of
/// investigators"*.
fn reject_untrackable_usage_limit(
    source: AbilitySource,
    code: &CardCode,
    address: &AbilityAddress,
    usage_limit: Option<UsageLimit>,
) -> Result<(), Cow<'static, str>> {
    if usage_limit.is_none() {
        return Ok(());
    }
    // The second untrackable shape (#772): a **granted** ability. The counter is
    // keyed by printed index — `BTreeMap<u8, _>`, because a JSON object key is a
    // string and an enum key does not survive the wire — so a granted ability
    // has nowhere to record a use. Refused for the same reason as the first
    // shape: silently uncapping a printed limit is worse than saying so. No
    // corpus grant prints one; the Parlor 01115's Parley is unlimited.
    if address.printed_index().is_none() {
        return Err(format!(
            "ActivateAbility: {code}'s granted ability at {address:?} carries a usage limit, but \
             the per-instance usage counter is keyed by printed index; TODO(#829): a granted \
             ability with a printed limit needs its own counter key"
        )
        .into());
    }
    if source.instance().is_some() {
        return Ok(());
    }
    Err(format!(
        "ActivateAbility: {code}'s ability carries a usage limit, but {source:?} has no card \
         instance to record uses against (usage state lives on in-play instances); \
         TODO(#699): usage limits on a location / act / agenda need a state-level counter"
    )
    .into())
}

/// Reject a cost that has to be paid *by the source* when the source has no card
/// instance to pay it with — `Exhaust`, `SpendUses` and `DiscardSelf` on a
/// location or an enemy.
///
/// Refused at validation rather than at payment so the turn menu never offers
/// it: `pay_activation_costs` addresses these costs through a `CardInPlay`
/// (#706), and a location has none. No corpus card needs one — the Parley
/// abilities cost cards, clues or resources (Herman Collins 01138 *"Choose and
/// discard 4 cards from your hand"*, Peter Warren 01139 *"Spend 2 clues"*,
/// Victoria Devereux 01140 and Mob Enforcer 01101 *"Spend … resources"*), all
/// investigator-side. Deliberately unlifted with no tracking issue (YAGNI, as
/// with [`reject_incompatible_costs`]); whoever prints such a card files one.
fn reject_source_costs_without_an_instance(
    source: AbilitySource,
    code: &CardCode,
    costs: &[Cost],
) -> Result<(), Cow<'static, str>> {
    if source.instance().is_some() {
        return Ok(());
    }
    let Some(cost) = costs.iter().find(|c| {
        matches!(
            c,
            Cost::Exhaust | Cost::SpendUses { .. } | Cost::DiscardSelf
        )
    }) else {
        return Ok(());
    };
    Err(format!(
        "ActivateAbility: {code}'s {cost:?} cost must be paid by the source, but {source:?} has \
         no card instance to pay it with"
    )
    .into())
}

/// Pure-validation peer to [`activate_ability`](abilities::activate_ability). Mirrors
/// [`check_play_card`]: validation block lifted verbatim, no behavior
/// change at the call site.
///
/// Returns `Ok(ActivateCheckResult)` if the ability is currently
/// activatable, `Err(reason)` otherwise. Does not mutate state.
pub(crate) fn check_activate_ability(
    state: &GameState,
    investigator: InvestigatorId,
    source: AbilitySource,
    address: &AbilityAddress,
) -> Result<ActivateCheckResult, Cow<'static, str>> {
    // Whether the investigator is Active is asked by the step (action-cost
    // abilities) and the initiation gate (every ability); this lookup only keeps
    // the missing-investigator refusal first and in this validator's voice.
    if !state.investigators.contains_key(&investigator) {
        return Err(
            format!("ActivateAbility: investigator {investigator:?} is not in state").into(),
        );
    }
    // Which sources exist is the reachability predicate's answer, and it is the
    // same one the turn-menu enumerator lists from (#707). Addressed by
    // identity, never by position: the position a validation computes is stale
    // the moment a cost removes the source (#706).
    let source_card = ability_source::resolve(state, investigator, source)?;
    let source_code = source_card.code().clone();
    let source_exhausted = source_card.exhausted();

    // Invariant: `resolve_activated_ability` currently returns only `Ok(...)`
    // (success) or `Err(EngineOutcome::Rejected { ... })` (validation failure).
    // If a future PR extends it to return `AwaitingInput` (e.g. for an ability
    // requiring target selection during validation), this `unreachable!()` will
    // panic; the validator's caller chain in `activate_ability` would need to be
    // redesigned to thread the `AwaitingInput` outcome back through
    // `check_activate_ability`'s `Result` shape. Mirrors the same invariant
    // comment on `resolve_play_target` in `check_play_card`.
    let ActivatedAbility {
        action_cost,
        designator,
        costs,
        effect,
        usage_limit,
    } = match abilities::resolve_activated_ability(state, source, &source_code, address) {
        Ok(v) => v,
        Err(EngineOutcome::Rejected { reason }) => return Err(reason),
        Err(other) => {
            unreachable!("resolve_activated_ability returned non-Rejected outcome: {other:?}")
        }
    };
    reject_untrackable_usage_limit(source, &source_code, address, usage_limit)?;

    // Gate: branch on action_cost now that we have it.
    // Fast abilities (action_cost == 0) may be used at any player window.
    let active_during_investigation =
        state.phase == Phase::Investigation && state.active_investigator == Some(investigator);
    let in_permissive_window = state
        .top_window()
        .is_some_and(|w| w.permits_fast(investigator));
    if action_cost > 0 {
        // Action-cost ability: requires Investigation phase + active investigator.
        if !active_during_investigation {
            return Err(format!(
                "ActivateAbility: action-cost ability requires Investigation phase + \
                 active investigator (phase was {:?}, active {:?})",
                state.phase, state.active_investigator,
            )
            .into());
        }
    } else {
        // Fast ability: active during Investigation OR permissive window.
        if !active_during_investigation && !in_permissive_window {
            return Err(
                "ActivateAbility: Fast ability requires either active investigator \
                         during Investigation, or an open window whose fast_actors permits \
                         this investigator"
                    .into(),
            );
        }
    }

    // An action-cost ability is taken as an action, so whether it can be
    // afforded is the step's answer: its printed cost plus any `ExtraActionCost`
    // surcharge on the class its bold designator names (#754). Official FAQ:
    // *"Abilities with a bold action designator (like Fight, Evade or
    // Investigate) count as an action of that type."* Asking the step means the
    // surcharge refused on here is the one taking it charges. A fast ability
    // takes no action and never asks: the same ruling taxes *fast* designated
    // abilities too, which no corpus card can reach (#759).
    if action_cost > 0 {
        let description = ActionDescription::activate(source, action_cost, designator.clone());
        take::check(state, investigator, &description)
            .map_err(|why| Cow::from(format!("ActivateAbility: {why}")))?;
    }

    reject_incompatible_costs(&costs)?;
    reject_source_costs_without_an_instance(source, &source_code, &costs)?;
    // Before the gate: a designated ability's change-state question is this
    // check's (`initiation::performs_an_action`).
    check_activation_target_available(state, investigator, designator.as_ref(), &effect)?;
    let candidate = ResolutionCandidate::new(
        source_code,
        investigator,
        address.clone(),
        CandidateSource::Ability(source),
    );
    initiation::check(state, &candidate, InitiationKind::Activated)
        .map_err(|refusal| activation_refusal(refusal, &candidate.code))?;

    Ok(ActivateCheckResult {
        candidate,
        action_cost,
        designator,
        costs,
        effect,
        usage_limit,
        source_exhausted,
    })
}

#[cfg(test)]
mod tests;
