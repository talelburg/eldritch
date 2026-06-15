//! DSL effect evaluator.
//!
//! Walks an [`Effect`] tree and mutates [`GameState`] accordingly.
//! Bridges card declarations (DSL) and runtime gameplay.
//!
//! # Phase-3 PR-J scope
//!
//! Implements the leaf effects whose state requirements are already
//! met by the engine ([`Effect::GainResources`], [`Effect::DiscoverClue`])
//! and the simplest composition ([`Effect::Seq`]). The remaining
//! variants return [`EngineOutcome::Rejected`] with a TODO message
//! pointing at the issue or PR that fills them in:
//!
//! - [`Effect::Modify`] splits by scope. [`WhileInPlay`] and
//!   [`WhileInPlayDuring`] contributions are passive and surfaced
//!   by [`constant_skill_modifier`] from card abilities directly —
//!   reaching `apply_effect` with one of those means the card
//!   author put a constant-flavored modifier under a non-constant
//!   trigger, which rejects. [`ThisSkillTest`] is **pushed** into
//!   [`GameState::pending_skill_modifiers`] for the active skill
//!   test to consume via [`pending_skill_modifier`]; the skill-
//!   test handler drains it after `SkillTestEnded`. [`ThisTurn`]
//!   is not yet wired; rejects with TODO until a card or test
//!   demands the turn-scoped accumulator.
//!
//! [`WhileInPlay`]: crate::dsl::ModifierScope::WhileInPlay
//! [`WhileInPlayDuring`]: crate::dsl::ModifierScope::WhileInPlayDuring
//! [`ThisSkillTest`]: crate::dsl::ModifierScope::ThisSkillTest
//! [`ThisTurn`]: crate::dsl::ModifierScope::ThisTurn
//! [`GameState::pending_skill_modifiers`]: crate::state::GameState::pending_skill_modifiers
//! - [`Effect::If`] evaluates [`Condition::SkillTestKind`] against
//!   the in-flight test's `kind`. [`SkillTest`](crate::dsl::Condition::SkillTest)
//!   isn't yet wired — the outcome isn't snapshotted onto state, and
//!   inside an [`Trigger::OnSkillTestResolution`] effect the trigger
//!   itself gates outcome, so the condition is redundant there.
//! - [`Effect::ForEach`] dispatches but the
//!   [`InvestigatorTargetSet`](crate::dsl::InvestigatorTargetSet)
//!   resolver ("at controller location", "all investigators")
//!   relies on per-target context that's not yet wired through.
//! - [`Effect::ChooseOne`] needs an `AwaitingInput` round-trip with
//!   [`ResolveInput`](crate::PlayerAction::ResolveInput); resume
//!   plumbing lands with the [`ChoiceResolver`](https://github.com/talelburg/eldritch/issues/19)
//!   alongside skill-test resolution (#49).
//!
//! # State-mutation contract
//!
//! `apply_effect` follows the same validate-first / mutate-second
//! pattern the existing dispatch handlers use: if the effect can't
//! resolve cleanly, return [`EngineOutcome::Rejected`] with no state
//! change and no events pushed. The outer apply loop's belt-and-
//! suspenders `events.clear()` on rejection backs this up.

use crate::card_registry::CardRegistry;
use crate::dsl::{
    Condition, Effect, InvestigatorTarget, LocationTarget, ModifierScope, SkillTestKind, Stat,
    Trigger,
};
use crate::event::Event;
use crate::state::{GameState, InvestigatorId, SkillKind};

use super::outcome::EngineOutcome;
use super::Cx;

/// Per-evaluation context the effect needs to resolve targets and
/// reference in-flight game state (current skill test, etc.).
///
/// Phase-3 minimal. Grows fields as effects demand them — current
/// skill test (for [`SkillTest`](crate::dsl::Condition::SkillTest)
/// condition), current target (for [`Effect::ForEach`] body),
/// reaction-window context (for `OnEvent` triggers), etc. Keep the
/// surface narrow and add fields only when an effect's evaluator
/// actually reads them.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct EvalContext {
    /// The investigator whose card-effect we're resolving — the
    /// "you" in card text. Resolves [`InvestigatorTarget::You`]
    /// and [`LocationTarget::YourLocation`].
    pub controller: crate::state::InvestigatorId,
    /// The in-play card-instance that triggered this effect, if any.
    /// Set by [`activate_ability`](crate::engine) so pushed
    /// [`PendingSkillModifier`](crate::state::PendingSkillModifier)
    /// entries can name their source (for replay clarity and future
    /// limit-once-per-test logic). `None` for evaluations not
    /// originating from a specific in-play instance (events played
    /// from hand, scenario forced effects, …).
    pub source: Option<crate::state::CardInstanceId>,
    /// The just-resolved skill test's failure margin, set only while the
    /// skill-test driver runs an [`Effect::SkillTest`]'s `on_fail`. Read
    /// by [`Effect::ForEachPointFailed`]. `None` outside that window.
    pub failed_by: Option<u8>,
    /// The clue count a before-timing discovery interrupt is replacing,
    /// set only while resolving an `EventPattern::WouldDiscoverClues`
    /// ability's effect (so the card-local "discard that many" Native
    /// reads it). `None` outside that window. Mirrors `failed_by`.
    pub clue_discovery_count: Option<u8>,
    /// The attacking enemy bound while resolving an
    /// `EnemyAttackDamagedSelf` reaction, so the card-local
    /// `Effect::Native` retaliate can name it. `None` outside that
    /// window. Mirrors `failed_by` / `clue_discovery_count`. (C5b #237.)
    pub attacking_enemy: Option<crate::state::EnemyId>,
}

impl EvalContext {
    /// Construct a context for the given controller with no source
    /// card. Use [`for_controller_with_source`](Self::for_controller_with_source)
    /// when the effect originates from a specific in-play instance.
    #[must_use]
    pub fn for_controller(controller: crate::state::InvestigatorId) -> Self {
        Self {
            controller,
            source: None,
            failed_by: None,
            clue_discovery_count: None,
            attacking_enemy: None,
        }
    }

    /// Construct a context for an effect triggered from a specific
    /// in-play card instance. Used by
    /// [`activate_ability`](crate::engine) so pushed
    /// `PendingSkillModifier`s carry their source.
    #[must_use]
    pub fn for_controller_with_source(
        controller: crate::state::InvestigatorId,
        source: crate::state::CardInstanceId,
    ) -> Self {
        Self {
            controller,
            source: Some(source),
            failed_by: None,
            clue_discovery_count: None,
            attacking_enemy: None,
        }
    }
}

/// Apply an effect tree to the state.
///
/// See module docs for the v0 scope and the validate-first
/// state-mutation contract.
///
/// **Stubbed-leaf cascade:** while any effect variant is stubbed
/// (returns `Rejected` with TODO), a [`Seq`](Effect::Seq) containing
/// it rejects as a unit even if other effects in the sequence are
/// implementable. That's correct given the stub semantics — the
/// evaluator can't safely run "the parts that work" because card
/// authors expect the whole sequence or nothing — but it means
/// gluing implemented + stubbed effects together still blocks on
/// the stubs.
pub(crate) fn apply_effect(cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext) -> EngineOutcome {
    match effect {
        Effect::GainResources { target, amount } => gain_resources(cx, eval_ctx, *target, *amount),
        Effect::DiscoverClue { from, count } => discover_clue(cx, eval_ctx, *from, *count),
        Effect::DealDamage { target, amount } => deal_damage_effect(cx, eval_ctx, *target, *amount),
        Effect::DealHorror { target, amount } => deal_horror_effect(cx, eval_ctx, *target, *amount),
        Effect::Seq(effects) => apply_seq(cx, effects, eval_ctx),
        Effect::Modify { stat, delta, scope } => modify(cx, eval_ctx, *stat, *delta, *scope),
        Effect::If {
            condition,
            then,
            else_,
        } => apply_if(cx, eval_ctx, condition, then, else_.as_deref()),
        Effect::ForEach { .. } => awaiting_input_stub("ForEach"),
        Effect::ChooseOne(_) => awaiting_input_stub("ChooseOne"),
        Effect::AdvanceCurrentAct => {
            use crate::engine::dispatch::act_agenda::{advance_act, request_resolution};
            if cx.state.act_deck.is_empty() {
                return EngineOutcome::Rejected {
                    reason: "AdvanceCurrentAct: no act deck is modeled".into(),
                };
            }
            match cx.state.act_deck[cx.state.act_index].resolution.clone() {
                Some(resolution) => request_resolution(cx.state, resolution),
                None => advance_act(cx),
            }
            EngineOutcome::Done
        }
        Effect::Native { tag } => {
            let Some(reg) = crate::card_registry::current() else {
                return EngineOutcome::Rejected {
                    reason: format!("Native effect {tag:?}: no card registry installed").into(),
                };
            };
            let Some(f) = (reg.native_effect_for)(tag) else {
                return EngineOutcome::Rejected {
                    reason: format!("Native effect {tag:?}: no handler registered").into(),
                };
            };
            f(cx, &eval_ctx)
        }
        Effect::ForEachPointFailed(body) => {
            let n = eval_ctx.failed_by.unwrap_or(0);
            for _ in 0..n {
                match apply_effect(cx, body, eval_ctx) {
                    EngineOutcome::Done => {}
                    other => return other,
                }
            }
            EngineOutcome::Done
        }
        Effect::SkillTest {
            skill,
            difficulty,
            on_success,
            on_fail,
        } => crate::engine::dispatch::skill_test::start_skill_test(
            cx,
            eval_ctx.controller,
            *skill,
            crate::dsl::SkillTestKind::Plain,
            i8::try_from(*difficulty).unwrap_or(i8::MAX),
            crate::state::SkillTestFollowUp::None,
            on_success.as_ref().map(|b| (**b).clone()),
            on_fail.as_ref().map(|b| (**b).clone()),
            eval_ctx.source,
        ),
        Effect::DiscardSelf => discard_self(cx, &eval_ctx),
        Effect::PutIntoThreatArea { code } => {
            crate::engine::dispatch::threat_area::place_in_threat_area(
                cx,
                eval_ctx.controller,
                crate::state::CardCode::new(code.clone()),
            );
            EngineOutcome::Done
        }
        Effect::Restrict(_) => EngineOutcome::Rejected {
            reason: "Effect::Restrict is a constant marker — inspected at decision points, \
                     never executed"
                .into(),
        },
    }
}

/// Resolve [`Effect::DiscardSelf`]: remove `eval_ctx.source` from
/// whichever threat area or location attachment holds it, push its code
/// to `encounter_discard`, and emit
/// [`Event::CardDiscarded`](crate::Event::CardDiscarded) with the
/// matching `from` zone. Rejects loudly if there is no source or the
/// instance is not found.
///
/// TODO: scoped to the two encounter zones (threat area / location
/// attachment → encounter discard). Extend to player-controlled zones
/// (cards in play → owner discard) when a player card first needs to
/// discard itself by source instance.
fn discard_self(cx: &mut Cx, eval_ctx: &EvalContext) -> EngineOutcome {
    use crate::event::Event;
    use crate::state::Zone;
    let Some(source) = eval_ctx.source else {
        return EngineOutcome::Rejected {
            reason: "DiscardSelf: no source instance in context".into(),
        };
    };
    // Locate first (immutable scan), then mutate — avoids a cross-field
    // borrow of `cx.state` while iterating one of its maps.
    let threat_owner = cx.state.investigators.iter().find_map(|(id, inv)| {
        inv.threat_area
            .iter()
            .position(|c| c.instance_id == source)
            .map(|pos| (*id, pos))
    });
    if let Some((inv_id, pos)) = threat_owner {
        let card = cx
            .state
            .investigators
            .get_mut(&inv_id)
            .expect("found above")
            .threat_area
            .remove(pos);
        cx.state.encounter_discard.push(card.code.clone());
        cx.events.push(Event::CardDiscarded {
            investigator: inv_id,
            code: card.code,
            from: Zone::ThreatArea,
        });
        return EngineOutcome::Done;
    }

    let att_owner = cx.state.locations.iter().find_map(|(id, loc)| {
        loc.attachments
            .iter()
            .position(|c| c.instance_id == source)
            .map(|pos| (*id, pos))
    });
    if let Some((loc_id, pos)) = att_owner {
        let card = cx
            .state
            .locations
            .get_mut(&loc_id)
            .expect("found above")
            .attachments
            .remove(pos);
        cx.state.encounter_discard.push(card.code.clone());
        // `CardDiscarded` carries an `investigator`; for a location
        // attachment, use the controller as the bookkeeping owner.
        cx.events.push(Event::CardDiscarded {
            investigator: eval_ctx.controller,
            code: card.code,
            from: Zone::LocationAttachment,
        });
        return EngineOutcome::Done;
    }

    EngineOutcome::Rejected {
        reason: format!(
            "DiscardSelf: source instance {source:?} not found in any threat area or location attachment"
        )
        .into(),
    }
}

/// Evaluate an [`Effect::If`].
///
/// Walks the [`Condition`], branches into `then` on hold or `else_`
/// otherwise (or [`EngineOutcome::Done`] when `else_` is absent).
/// Condition evaluation that needs context the engine can't supply
/// today (e.g. comparing against a stat snapshot not stored on
/// state) returns [`EngineOutcome::Rejected`] with a TODO message.
///
/// Inherits [`apply_seq`]'s partial-events-on-rejection caveat: a
/// `Rejected` returned by the branch passes through with whatever
/// events the branch already pushed. The structural fix lives at
/// the outer `apply` loop (TODO in `engine/mod.rs::apply`).
fn apply_if(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    condition: &Condition,
    then: &Effect,
    else_: Option<&Effect>,
) -> EngineOutcome {
    let holds = match eval_condition(cx.state, condition) {
        Ok(b) => b,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    if holds {
        apply_effect(cx, then, eval_ctx)
    } else if let Some(else_branch) = else_ {
        apply_effect(cx, else_branch, eval_ctx)
    } else {
        EngineOutcome::Done
    }
}

/// Resolve a [`Condition`] against the current state.
///
/// Returns `Err` for conditions that aren't expressible yet (the
/// state shape they'd query against doesn't exist) — the caller
/// turns those into [`EngineOutcome::Rejected`].
fn eval_condition(state: &GameState, condition: &Condition) -> Result<bool, String> {
    match condition {
        Condition::SkillTestKind(kind) => {
            let t = state.in_flight_skill_test.as_ref().ok_or_else(|| {
                "Condition::SkillTestKind but no skill test is in flight".to_owned()
            })?;
            Ok(t.kind == *kind)
        }
        Condition::SkillTest { outcome } => {
            // Inside an [`Trigger::OnSkillTestResolution`] effect, the
            // outcome is already gated by the trigger; using this
            // condition there is redundant. Outside that context
            // (e.g. an OnEvent reaction keying off `SkillTestSucceeded`),
            // the engine would need to snapshot the outcome onto
            // state, which it doesn't today. Reject with a TODO
            // pointing at the preferred trigger.
            Err(format!(
                "TODO: Condition::SkillTest {{ outcome: {outcome:?} }} not yet evaluated; \
                 prefer Trigger::OnSkillTestResolution for resolution-time effects, \
                 or wait for an OnEvent-based reaction model to surface past-test outcome."
            ))
        }
    }
}

/// Apply an [`Effect::Modify`].
///
/// Most scopes are passive contributions queried elsewhere:
///
/// - [`ModifierScope::WhileInPlay`] / [`ModifierScope::WhileInPlayDuring`]:
///   the constant-modifier query walks `cards_in_play` and reads
///   abilities directly. Reaching `apply_effect` with one of these
///   means a card author put a constant-flavored modifier under a
///   non-constant trigger (an `OnPlay`/`Activated` ability whose
///   effect *is* a `Modify` with constant scope), which doesn't fit
///   either path cleanly. Reject loudly so the card author notices.
/// - [`ModifierScope::ThisSkillTest`]: pushed onto
///   [`GameState::pending_skill_modifiers`]; consumed and drained
///   by the skill-test resolution flow.
/// - [`ModifierScope::ThisTurn`]: not yet wired; rejects with TODO
///   until a card or test demands it.
fn modify(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    stat: crate::dsl::Stat,
    delta: i8,
    scope: ModifierScope,
) -> EngineOutcome {
    match scope {
        ModifierScope::ThisSkillTest => {
            cx.state
                .pending_skill_modifiers
                .push(crate::state::PendingSkillModifier {
                    investigator: eval_ctx.controller,
                    stat,
                    delta,
                    source: eval_ctx.source,
                });
            EngineOutcome::Done
        }
        ModifierScope::WhileInPlay | ModifierScope::WhileInPlayDuring(_) => {
            EngineOutcome::Rejected {
                reason: format!(
                    "Modify with constant scope ({scope:?}) under a non-constant trigger isn't \
                     applied via the evaluator; declare it under Trigger::Constant so the \
                     constant-modifier query picks it up. Stat = {stat:?}, delta = {delta}."
                )
                .into(),
            }
        }
        ModifierScope::ThisTurn => EngineOutcome::Rejected {
            reason: "TODO(#102-followup): ThisTurn scope not yet wired; needs a turn-scoped \
                     accumulator that drains on TurnEnded."
                .into(),
        },
    }
}

/// Standard rejection message for effect variants whose evaluator
/// needs `AwaitingInput` plumbing (engine-side producer + `ResolveInput`
/// resume). Centralizes the message so the un-stub path is one grep.
/// Test-side seam is [`ChoiceResolver`](crate::test_support::ChoiceResolver).
fn awaiting_input_stub(name: &'static str) -> EngineOutcome {
    EngineOutcome::Rejected {
        reason: format!(
            "TODO: {name} evaluator needs AwaitingInput + ResolveInput resume; \
             no engine consumer has landed yet."
        )
        .into(),
    }
}

// ---- leaf-effect implementations ------------------------------

fn gain_resources(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: InvestigatorTarget,
    amount: u8,
) -> EngineOutcome {
    if amount == 0 {
        // Zero-amount gain is a no-op: no state change, no event,
        // no target resolution. Matches DiscoverClue's zero-count
        // behavior and the rulebook intuition that "gain 0 resources"
        // isn't a state change worth narrating.
        return EngineOutcome::Done;
    }
    let target_id = match resolve_investigator_target(cx.state, eval_ctx, target) {
        Ok(id) => id,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    // Validate-first: confirm the investigator exists in state before
    // we touch anything. The "active" target may resolve to None if
    // outside the Investigation phase; that's a reject, not a panic.
    if !cx.state.investigators.contains_key(&target_id) {
        return EngineOutcome::Rejected {
            reason: format!("GainResources: investigator {target_id:?} is not in the state").into(),
        };
    }
    crate::engine::dispatch::cards::grant_resources(cx, target_id, amount);
    EngineOutcome::Done
}

fn discover_clue(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    from: LocationTarget,
    count: u8,
) -> EngineOutcome {
    if count == 0 {
        // Zero-count is a no-op rather than an error; some card text
        // can resolve to "discover N clues" where N == 0 (e.g. via a
        // future Modify-on-effect that reduces count). Don't reject;
        // just emit nothing.
        return EngineOutcome::Done;
    }

    // Resolve the source location.
    let location_id = match resolve_location_target(cx.state, eval_ctx, from) {
        Ok(id) => id,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };

    // Validate-first: collect the data we need to mutate without
    // mutating yet, so a missing-investigator or empty-location case
    // doesn't leave state half-modified.
    let Some(location) = cx.state.locations.get(&location_id) else {
        return EngineOutcome::Rejected {
            reason: format!("DiscoverClue: location {location_id:?} is not in the state").into(),
        };
    };
    if location.clues == 0 {
        // A discover effect against an empty location is a no-op per
        // the rulebook ("if there are no clues, no clues are
        // discovered"). Don't reject; just do nothing.
        return EngineOutcome::Done;
    }

    if !cx.state.investigators.contains_key(&eval_ctx.controller) {
        return EngineOutcome::Rejected {
            reason: format!(
                "DiscoverClue: controller {:?} is not in the state",
                eval_ctx.controller
            )
            .into(),
        };
    }

    // Before-timing clue-discovery interrupt (Cover Up 01007, C5a #236).
    // Offer the controller a chance to replace this discovery iff they
    // control a card with a `WouldDiscoverClues` reaction at the
    // controller's own location ("at your location"). No registry (unit
    // context) or no eligible card → fall through to the normal discovery.
    //
    // The `card.clues > 0` gate below is NOT generic to the trigger point:
    // it is a single-consumer stand-in for the eligibility rule "a
    // triggered ability can only be initiated if its effect has the
    // potential to change the game state" (RR p.2), which the engine does
    // not yet model generically (no reaction window checks potential). For
    // Cover Up specifically, an emptied card sits in the threat area until
    // game end, so without this gate the engine would prompt a never-useful
    // interrupt on every discovery for the rest of the game.
    // TODO(#212): when a second `WouldDiscoverClues` card lands (one whose
    // potential isn't "holds clues to discard"), lift this into a
    // card-provided per-ability "has potential" predicate rather than a
    // hardcoded clue check here.
    if let Some(reg) = crate::card_registry::current() {
        let at_your_location = cx
            .state
            .investigators
            .get(&eval_ctx.controller)
            .and_then(|i| i.current_location)
            == Some(location_id);
        if at_your_location {
            // Read-only scan first (collect the hit), then set the pending
            // state — keeps the immutable borrow of the investigator
            // disjoint from the later mutable write.
            let hit = cx
                .state
                .investigators
                .get(&eval_ctx.controller)
                .and_then(|inv| {
                    inv.controlled_card_instances().find_map(|card| {
                        // Single-consumer eligibility stand-in (see above).
                        if card.clues == 0 {
                            return None;
                        }
                        let abilities = (reg.abilities_for)(&card.code)?;
                        let idx = abilities.iter().position(|a| {
                            matches!(
                                &a.trigger,
                                crate::dsl::Trigger::OnEvent {
                                    pattern: crate::dsl::EventPattern::WouldDiscoverClues,
                                    timing: crate::dsl::EventTiming::Before,
                                }
                            )
                        })?;
                        Some((card.instance_id, idx))
                    })
                });
            if let Some((source, ability_index)) = hit {
                // TODO(#212): `count` is the *requested* count, not the
                // capped/actually-discoverable one. Per the card, "discard
                // that many" means the number you would actually discover
                // (`min(count, location.clues)`). They coincide in Slice 1
                // (Investigate is count=1 and the empty-location early-return
                // above guarantees >= 1 clue present), so this is latent
                // until a multi-count or sub-availability discovery is
                // reachable.
                cx.state.clue_interrupt_pending = Some(crate::state::ClueInterruptPending {
                    controller: eval_ctx.controller,
                    location: location_id,
                    count,
                    source,
                    ability_index,
                });
                return EngineOutcome::AwaitingInput {
                    request: crate::engine::outcome::InputRequest {
                        prompt: "You would discover clue(s). Use the interrupt to discard that \
                                 many from the source card instead? Confirm = replace, \
                                 Skip = discover normally."
                            .to_owned(),
                    },
                    resume_token: crate::engine::outcome::ResumeToken(0),
                };
            }
        }
    }

    perform_discovery(cx, location_id, count, eval_ctx.controller);
    EngineOutcome::Done
}

/// Move `count` clues (capped at availability) from `location_id` to
/// `controller`, emitting `CluePlaced` + `LocationCluesChanged`. The
/// committed mutation half of [`discover_clue`], factored out so the
/// clue-discovery interrupt's `Skip` resume can perform the deferred
/// discovery (C5a #236). Caller guarantees both ids exist and the
/// location has clues.
pub(crate) fn perform_discovery(
    cx: &mut Cx,
    location_id: crate::state::LocationId,
    count: u8,
    controller: crate::state::InvestigatorId,
) {
    let location = cx
        .state
        .locations
        .get(&location_id)
        .expect("location exists");
    // Cap the discovery at the location's actual clue count — a card
    // can't pull more clues than exist.
    let actually_taken = count.min(location.clues);
    let new_location_count = location.clues - actually_taken;
    cx.state
        .locations
        .get_mut(&location_id)
        .expect("checked above")
        .clues = new_location_count;
    let investigator = cx
        .state
        .investigators
        .get_mut(&controller)
        .expect("checked above");
    investigator.clues = investigator.clues.saturating_add(actually_taken);
    cx.events.push(Event::CluePlaced {
        investigator: controller,
        count: actually_taken,
    });
    cx.events.push(Event::LocationCluesChanged {
        location: location_id,
        new_count: new_location_count,
    });
}

fn deal_damage_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: InvestigatorTarget,
    amount: u8,
) -> EngineOutcome {
    if amount == 0 {
        return EngineOutcome::Done;
    }
    let target_id = match resolve_investigator_target(cx.state, eval_ctx, target) {
        Ok(id) => id,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    if !cx.state.investigators.contains_key(&target_id) {
        return EngineOutcome::Rejected {
            reason: format!("DealDamage: investigator {target_id:?} is not in the state").into(),
        };
    }
    crate::engine::dispatch::elimination::take_damage(cx, target_id, amount);
    EngineOutcome::Done
}

fn deal_horror_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: InvestigatorTarget,
    amount: u8,
) -> EngineOutcome {
    if amount == 0 {
        return EngineOutcome::Done;
    }
    let target_id = match resolve_investigator_target(cx.state, eval_ctx, target) {
        Ok(id) => id,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    if !cx.state.investigators.contains_key(&target_id) {
        return EngineOutcome::Rejected {
            reason: format!("DealHorror: investigator {target_id:?} is not in the state").into(),
        };
    }
    crate::engine::dispatch::elimination::take_horror(cx, target_id, amount);
    EngineOutcome::Done
}

fn apply_seq(cx: &mut Cx, effects: &[Effect], eval_ctx: EvalContext) -> EngineOutcome {
    // Stop at the first non-Done outcome. A Rejected mid-Seq leaves
    // earlier effects committed *within this apply*, but the `apply`
    // boundary rolls the whole call back to its pre-dispatch snapshot on
    // Rejected (see engine/mod.rs::apply "Handler contract"), so the
    // partial mutation never escapes. The AwaitingInput-resume note below
    // still stands.
    //
    // **AwaitingInput resume:** when ChooseOne et al. land and start
    // returning AwaitingInput mid-Seq, this loop will need to track
    // a resume token + remaining-effects continuation. Today
    // AwaitingInput is unreachable here (no implemented variant
    // produces it), so the simple early-return is correct for v0.
    for effect in effects {
        let outcome = apply_effect(cx, effect, eval_ctx);
        if !matches!(outcome, EngineOutcome::Done) {
            return outcome;
        }
    }
    EngineOutcome::Done
}

// ---- target resolution ----------------------------------------

/// Resolve an [`InvestigatorTarget`] to a concrete id given the
/// current evaluation context.
///
/// **`Active` semantics:** rejects when no investigator is active
/// (outside the Investigation phase). Card authors reaching for
/// `Active` from a Mythos- or Enemy-phase reaction will silently
/// fail until reaction windows wire an active-investigator-equivalent
/// into `EvalContext`. Use [`InvestigatorTarget::You`] for
/// "the player who triggered this" — it doesn't depend on phase.
fn resolve_investigator_target(
    state: &GameState,
    ctx: EvalContext,
    target: InvestigatorTarget,
) -> Result<crate::state::InvestigatorId, &'static str> {
    match target {
        InvestigatorTarget::You => Ok(ctx.controller),
        InvestigatorTarget::Active => state
            .active_investigator
            .ok_or("InvestigatorTarget::Active but no active investigator (outside Investigation)"),
        InvestigatorTarget::ChosenByController => {
            // Same shape as ChooseOne — needs AwaitingInput + a
            // ResolveInput round-trip carrying the chosen id. No
            // engine consumer landed yet.
            Err("InvestigatorTarget::ChosenByController requires AwaitingInput plumbing")
        }
    }
}

fn resolve_location_target(
    state: &GameState,
    ctx: EvalContext,
    target: LocationTarget,
) -> Result<crate::state::LocationId, &'static str> {
    match target {
        LocationTarget::YourLocation => state
            .investigators
            .get(&ctx.controller)
            .and_then(|i| i.current_location)
            .ok_or("LocationTarget::YourLocation but the controller is between locations"),
        LocationTarget::ChosenByController => {
            Err("LocationTarget::ChosenByController requires AwaitingInput plumbing")
        }
        LocationTarget::TestedLocation => state
            .in_flight_skill_test
            .as_ref()
            .ok_or("LocationTarget::TestedLocation but no skill test is in flight")
            .and_then(|t| {
                t.tested_location.ok_or(
                    "LocationTarget::TestedLocation but the test's location is unset \
                     (investigator was between locations at test start)",
                )
            }),
    }
}

// ---- constant-modifier query ----------------------------------

/// Sum the constant skill-modifier contributions from every card in
/// `controller`'s `cards_in_play` that apply to a skill test of the
/// given `skill` + `kind`.
///
/// Walks each in-play card code, looks up its abilities via the
/// supplied [`CardRegistry`], and sums every [`Effect::Modify`] under
/// a [`Trigger::Constant`] ability where the stat matches `skill` and
/// the scope is either:
///
/// - [`ModifierScope::WhileInPlay`] — applies to any skill test (Holy
///   Rosary's unqualified +1 willpower).
/// - [`ModifierScope::WhileInPlayDuring(k)`](ModifierScope::WhileInPlayDuring)
///   where `k == kind` — Magnifying Glass's +1 intellect *while
///   investigating* contributes during `SkillTestKind::Investigate`
///   but NOT during `SkillTestKind::Plain`.
///
/// Other scopes (`ThisSkillTest`, `ThisTurn`) are not constant
/// contributions and are skipped here.
///
/// # Why only the controller's cards
///
/// Constant modifiers on player cards are scoped to their controller
/// ("you get +1 willpower"). Solo coincides with "every investigator"
/// but multi-investigator does not — a controller's Holy Rosary must
/// not give every other investigator +1 willpower. Cards that DO
/// modify all investigators ("each investigator at your location gets
/// +1 …") need a new `ModifierScope` variant; out of scope here.
///
/// # What this deliberately does NOT cover
///
/// - **Conditional constants** (`Effect::If` under a `Trigger::Constant`):
///   not yet wired; this helper ignores them.
/// - **Commit-time bonuses** (`ModifierScope::ThisSkillTest`): not in
///   scope for constants; the skill-test commit window (#63) handles
///   those.
/// - **Ready/exhaust gating** on constant sources: most rulebook
///   constants apply regardless of the source asset's ready/exhaust
///   state. When a ready-gated constant card lands, filter on
///   `in_play.exhausted` here.
/// - **Unimplemented cards**: cards in `cards_in_play` whose code
///   isn't in the registry's `abilities_for` return None; we skip
///   them silently. In practice the deck-import gate (Phase 9) keeps
///   unimplemented codes out of play, so silent-skip is the safest
///   v1 behavior.
#[must_use]
pub fn constant_skill_modifier(
    state: &GameState,
    registry: &CardRegistry,
    controller: InvestigatorId,
    skill: SkillKind,
    kind: SkillTestKind,
) -> i8 {
    sum_constant_modify(
        state,
        registry,
        controller,
        |scope| scope_applies(scope, kind),
        |stat| stat_matches_skill(stat, skill),
    )
}

/// Sum a controller's *unconditional* constant modifiers to `stat`: only
/// [`ModifierScope::WhileInPlay`] `Trigger::Constant` `Effect::Modify`
/// abilities matching that exact stat (Beat Cop's always-on
/// `+1 [combat]`, or a future "+N max health"). Excludes
/// [`WhileInPlayDuring`](ModifierScope::WhileInPlayDuring) (which need a
/// skill-test context) and every non-constant scope.
///
/// Used by prey ranking ([#270]): a prey instruction like
/// `Highest [combat]` or "Lowest remaining health" compares *modified*
/// values (Rules Reference p.18 Modifiers, p.12 remaining health), but
/// resolves outside any skill test, so only always-on modifiers apply.
///
/// [#270]: https://github.com/talelburg/eldritch/issues/270
#[must_use]
pub fn unconditional_constant_stat_modifier(
    state: &GameState,
    registry: &CardRegistry,
    controller: InvestigatorId,
    stat: Stat,
) -> i8 {
    sum_constant_modify(
        state,
        registry,
        controller,
        |scope| matches!(scope, ModifierScope::WhileInPlay),
        |s| s == stat,
    )
}

/// A location's **effective shroud**: its printed `shroud` plus every
/// `Stat::Shroud` `Modify(WhileInPlay)` constant ability on its
/// attachments (Obscuring Fog 01168's `+2`). Clamped to `[0, u8::MAX]`.
/// Read by `investigate` in place of the raw printed shroud.
#[must_use]
pub fn effective_shroud(registry: &CardRegistry, location: &crate::state::Location) -> u8 {
    let mut delta: i32 = 0;
    for att in &location.attachments {
        let Some(abilities) = (registry.abilities_for)(&att.code) else {
            continue;
        };
        for ability in &abilities {
            if ability.trigger != Trigger::Constant {
                continue;
            }
            if let Effect::Modify {
                stat: Stat::Shroud,
                delta: d,
                scope: ModifierScope::WhileInPlay,
            } = &ability.effect
            {
                delta += i32::from(*d);
            }
        }
    }
    let total = i32::from(location.shroud) + delta;
    u8::try_from(total.clamp(0, i32::from(u8::MAX))).unwrap_or(u8::MAX)
}

/// Whether `investigator` is currently forbidden from playing a card of
/// `card_type` by an active `Restriction::CannotPlay` constant ability on
/// any of their controlled instances (Dissonant Voices 01165: assets and
/// events). Checked in `play_card` validation.
#[must_use]
pub fn play_is_prohibited(
    state: &GameState,
    registry: &CardRegistry,
    investigator: InvestigatorId,
    card_type: crate::card_data::CardType,
) -> bool {
    let Some(inv) = state.investigators.get(&investigator) else {
        return false;
    };
    inv.controlled_card_instances().any(|c| {
        (registry.abilities_for)(&c.code)
            .into_iter()
            .flatten()
            .any(|a| {
                a.trigger == Trigger::Constant
                    && matches!(
                        &a.effect,
                        Effect::Restrict(crate::dsl::Restriction::CannotPlay(t)) if *t == card_type
                    )
            })
    })
}

/// The extra action cost `investigator` pays to perform `action_class`,
/// plus the `first_each_round` source instances to mark spent on commit.
///
/// Sums `Restriction::ExtraActionCost` deltas (1 each) from active
/// `Trigger::Constant` abilities on the investigator's controlled
/// instances whose `actions` include `action_class` (Frozen in Fear
/// 01164: move / fight / evade). A `first_each_round` source already in
/// `action_surcharge_spent_this_round` contributes 0; the returned
/// instance list is the set the caller marks spent **after** the action
/// commits (so cost-peek stays read-only for validate-first). Always-on
/// (`first_each_round == false`) surcharges always contribute and are not
/// returned for marking.
#[must_use]
pub fn pending_action_surcharge(
    state: &GameState,
    registry: &CardRegistry,
    investigator: InvestigatorId,
    action_class: crate::dsl::ActionClass,
) -> (u8, Vec<crate::state::CardInstanceId>) {
    use crate::dsl::Restriction;
    let Some(inv) = state.investigators.get(&investigator) else {
        return (0, Vec::new());
    };
    let mut extra: u8 = 0;
    let mut to_mark = Vec::new();
    for card in inv.controlled_card_instances() {
        let Some(abilities) = (registry.abilities_for)(&card.code) else {
            continue;
        };
        for a in &abilities {
            if a.trigger != Trigger::Constant {
                continue;
            }
            let Effect::Restrict(Restriction::ExtraActionCost {
                actions,
                first_each_round,
            }) = &a.effect
            else {
                continue;
            };
            if !actions.contains(&action_class) {
                continue;
            }
            if *first_each_round {
                if inv
                    .action_surcharge_spent_this_round
                    .contains(&card.instance_id)
                {
                    continue;
                }
                to_mark.push(card.instance_id);
            }
            extra = extra.saturating_add(1);
        }
    }
    (extra, to_mark)
}

/// Shared core of [`constant_skill_modifier`] and
/// [`unconditional_constant_stat_modifier`]: sum the `delta` of every
/// `Trigger::Constant` `Effect::Modify` on `controller`'s cards in play
/// whose scope and stat both satisfy the given predicates. Silently skips
/// cards whose code the registry can't resolve (same policy as the
/// callers — the deck-import gate keeps unimplemented codes out of play).
fn sum_constant_modify(
    state: &GameState,
    registry: &CardRegistry,
    controller: InvestigatorId,
    scope_ok: impl Fn(ModifierScope) -> bool,
    stat_ok: impl Fn(Stat) -> bool,
) -> i8 {
    let Some(inv) = state.investigators.get(&controller) else {
        return 0;
    };
    let mut total: i8 = 0;
    for in_play in &inv.cards_in_play {
        let Some(abilities) = (registry.abilities_for)(&in_play.code) else {
            continue;
        };
        for ability in &abilities {
            if ability.trigger != Trigger::Constant {
                continue;
            }
            let Effect::Modify { stat, delta, scope } = &ability.effect else {
                continue;
            };
            if scope_ok(*scope) && stat_ok(*stat) {
                total = total.saturating_add(*delta);
            }
        }
    }
    total
}

/// Sum the [`PendingSkillModifier`](crate::state::PendingSkillModifier)
/// contributions queued for `controller`'s in-flight skill test
/// against `skill`.
///
/// These are the modifiers pushed by
/// [`ModifierScope::ThisSkillTest`]-scoped `Effect::Modify`
/// evaluations from activated / triggered abilities (Hyperawareness's
/// `[fast] Spend 1 resource: You get +1 [intellect] for this skill
/// test` is the canonical example). The skill-test handler drains
/// the entries for the resolving investigator after
/// [`Event::SkillTestEnded`] fires;
/// stale entries from a prior test never leak into the next.
#[must_use]
pub fn pending_skill_modifier(
    state: &GameState,
    controller: InvestigatorId,
    skill: SkillKind,
) -> i8 {
    let mut total: i8 = 0;
    for pending in &state.pending_skill_modifiers {
        if pending.investigator != controller {
            continue;
        }
        if stat_matches_skill(pending.stat, skill) {
            total = total.saturating_add(pending.delta);
        }
    }
    total
}

/// Whether a constant-trigger [`ModifierScope`] contributes to a
/// skill test of the given [`SkillTestKind`].
///
/// [`WhileInPlay`](ModifierScope::WhileInPlay) is unqualified — it
/// applies to every test. [`WhileInPlayDuring`](ModifierScope::WhileInPlayDuring)
/// only fires when the in-flight test's kind matches the scope's
/// kind. Non-constant scopes never apply through this query path.
fn scope_applies(scope: ModifierScope, kind: SkillTestKind) -> bool {
    match scope {
        ModifierScope::WhileInPlay => true,
        ModifierScope::WhileInPlayDuring(k) => k == kind,
        ModifierScope::ThisSkillTest | ModifierScope::ThisTurn => false,
    }
}

/// Whether a DSL [`Stat`] refers to the same axis as a state-side
/// [`SkillKind`]. Non-skill stats ([`Stat::MaxHealth`] / [`Stat::MaxSanity`])
/// never match.
fn stat_matches_skill(stat: Stat, skill: SkillKind) -> bool {
    matches!(
        (stat, skill),
        (Stat::Willpower, SkillKind::Willpower)
            | (Stat::Intellect, SkillKind::Intellect)
            | (Stat::Combat, SkillKind::Combat)
            | (Stat::Agility, SkillKind::Agility)
    )
}

/// Find the in-play location whose printed code equals `code`; `None` if
/// no in-play location carries it. Public so card-local
/// [`Effect::Native`] handlers can resolve a board location by its
/// printed code.
pub fn location_id_by_code(state: &GameState, code: &str) -> Option<crate::state::LocationId> {
    state
        .locations
        .iter()
        .find(|(_, loc)| loc.code.as_str() == code)
        .map(|(id, _)| *id)
}

#[cfg(test)]
mod tests {
    use crate::card_registry::CardRegistry;
    use crate::dsl::{
        constant, deal_damage, deal_horror, discover_clue, for_each_point_failed, gain_resources,
        modify, on_play, seq, Ability, Effect, InvestigatorTarget, LocationTarget, ModifierScope,
        SkillTestKind, Stat,
    };
    use crate::event::Event;
    use crate::state::{
        CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, SkillKind,
    };
    use crate::test_support::{test_investigator, test_location, GameStateBuilder};
    use crate::{assert_event, assert_event_count, assert_no_event};

    use super::{
        apply_effect, constant_skill_modifier, effective_shroud,
        unconditional_constant_stat_modifier, EngineOutcome, EvalContext,
    };
    use crate::engine::Cx;

    fn ctx(id: u32) -> EvalContext {
        EvalContext::for_controller(InvestigatorId(id))
    }

    #[test]
    fn eval_context_defaults_clue_discovery_count_to_none() {
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        assert_eq!(ctx.clue_discovery_count, None);
    }

    #[test]
    fn gain_resources_increments_target_wallet_and_emits_event() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let resources_before = state.investigators[&id].resources;
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::You, 3),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].resources, resources_before + 3);
        assert_event!(
            events,
            Event::ResourcesGained { investigator, amount: 3 } if *investigator == id
        );
    }

    #[test]
    fn gain_resources_zero_amount_is_a_silent_noop() {
        // Symmetric with discover_clue_on_empty_location_is_a_silent_noop:
        // a zero-amount gain isn't a state change. Crucially, it also
        // skips target resolution, so an `Active` target with no
        // active investigator doesn't reject for amount=0.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let resources_before = state.investigators[&id].resources;
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::Active, 0),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].resources, resources_before);
        assert!(events.is_empty());
    }

    #[test]
    fn gain_resources_active_target_rejects_without_active_investigator() {
        // No active investigator (default phase is Mythos), so
        // InvestigatorTarget::Active should fail to resolve.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::Active, 1),
            ctx(1),
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn discover_clue_moves_one_clue_from_location_to_controller() {
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(10);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(loc_id);
        let mut location = test_location(10, "Study");
        location.clues = 3;

        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::YourLocation, 1),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.locations[&loc_id].clues, 2);
        assert_eq!(state.investigators[&inv_id].clues, 1);
        assert_event!(
            events,
            Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
        );
        assert_event!(
            events,
            Event::LocationCluesChanged { location, new_count: 2 } if *location == loc_id
        );
    }

    #[test]
    fn discover_clue_without_registry_discovers_normally() {
        // No registry installed (game-core unit context) → the interrupt
        // scan finds nothing → discovery proceeds exactly as before.
        // Regression guard for the seam's "fall through" path (C5a #236).
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(10);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(loc_id);
        let mut location = test_location(10, "Study");
        location.clues = 3;

        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::YourLocation, 1),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.clue_interrupt_pending.is_none());
        assert_eq!(state.locations[&loc_id].clues, 2);
        assert_eq!(state.investigators[&inv_id].clues, 1);
    }

    #[test]
    fn discover_clue_caps_at_location_clue_count() {
        // Card asks for 3 clues but the location only has 1 — take
        // what's there, no error.
        let loc_id = LocationId(10);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(loc_id);
        let mut location = test_location(10, "Study");
        location.clues = 1;

        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::YourLocation, 3),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.locations[&loc_id].clues, 0);
        assert_eq!(state.investigators[&InvestigatorId(1)].clues, 1);
        assert_event!(
            events,
            Event::CluePlaced {
                investigator: _,
                count: 1
            }
        );
    }

    #[test]
    fn discover_clue_on_empty_location_is_a_silent_noop() {
        // Per the rulebook: a discover-clue effect against an empty
        // location is a no-op, not a rejection.
        let loc_id = LocationId(10);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(loc_id);
        let location = test_location(10, "Study"); // 0 clues by default

        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::YourLocation, 1),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.locations[&loc_id].clues, 0);
        assert_eq!(state.investigators[&InvestigatorId(1)].clues, 0);
        assert_no_event!(events, Event::CluePlaced { .. });
    }

    #[test]
    fn discover_clue_rejects_when_controller_is_between_locations() {
        // "You" has no current_location — LocationTarget::
        // YourLocation can't resolve.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1)) // current_location = None
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::YourLocation, 1),
            ctx(1),
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(events.is_empty());
    }

    #[test]
    fn discover_clue_tested_location_resolves_to_in_flight_test_location() {
        // LocationTarget::TestedLocation reads
        // GameState::in_flight_skill_test.tested_location, regardless
        // of where the controller currently is. Set the controller's
        // current_location to a *different* location and confirm the
        // discover lands at the tested location.
        let tested = LocationId(20);
        let elsewhere = LocationId(30);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(elsewhere);
        let mut tested_loc = test_location(20, "Study");
        tested_loc.clues = 2;
        let elsewhere_loc = test_location(30, "Hall");

        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location(tested_loc)
            .with_location(elsewhere_loc)
            .build();
        state.in_flight_skill_test = Some(crate::state::InFlightSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Intellect,
            kind: SkillTestKind::Investigate,
            difficulty: 2,
            committed_by_active: Vec::new(),
            tested_location: Some(tested),
            follow_up: crate::state::SkillTestFollowUp::Investigate,
            on_fail: None,
            on_success: None,
            source: None,
            continuation: crate::state::FinishContinuation::AwaitingCommit,
        });
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::TestedLocation, 1),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.locations[&tested].clues, 1);
        assert_eq!(state.locations[&elsewhere].clues, 0);
        assert_eq!(state.investigators[&InvestigatorId(1)].clues, 1);
    }

    #[test]
    fn tested_location_rejects_without_in_flight_test() {
        // No in-flight skill test → TestedLocation can't resolve.
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(LocationId(10));
        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location({
                let mut l = test_location(10, "Study");
                l.clues = 1;
                l
            })
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::TestedLocation, 1),
            ctx(1),
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(events.is_empty());
    }

    // ---- Effect::If + Condition::SkillTestKind tests -------------

    fn state_with_in_flight_kind(kind: SkillTestKind) -> crate::state::GameState {
        let mut state = GameStateBuilder::new()
            .with_investigator({
                let mut inv = test_investigator(1);
                inv.current_location = Some(LocationId(10));
                inv
            })
            .with_location({
                let mut l = test_location(10, "Study");
                l.clues = 2;
                l
            })
            .build();
        state.in_flight_skill_test = Some(crate::state::InFlightSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Intellect,
            kind,
            difficulty: 2,
            committed_by_active: Vec::new(),
            tested_location: Some(LocationId(10)),
            follow_up: crate::state::SkillTestFollowUp::None,
            on_fail: None,
            on_success: None,
            source: None,
            continuation: crate::state::FinishContinuation::AwaitingCommit,
        });
        state
    }

    #[test]
    fn if_skill_test_kind_runs_then_branch_when_kind_matches() {
        use crate::dsl::{discover_clue, if_, Condition};
        let mut state = state_with_in_flight_kind(SkillTestKind::Investigate);
        let mut events = Vec::new();
        let effect = if_(
            Condition::SkillTestKind(SkillTestKind::Investigate),
            discover_clue(LocationTarget::TestedLocation, 1),
        );

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.locations[&LocationId(10)].clues, 1);
        assert_eq!(state.investigators[&InvestigatorId(1)].clues, 1);
    }

    #[test]
    fn if_skill_test_kind_skips_then_branch_when_kind_differs() {
        use crate::dsl::{discover_clue, if_, Condition};
        let mut state = state_with_in_flight_kind(SkillTestKind::Plain);
        let mut events = Vec::new();
        let effect = if_(
            Condition::SkillTestKind(SkillTestKind::Investigate),
            discover_clue(LocationTarget::TestedLocation, 1),
        );

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        // No-op: location clues unchanged, no events emitted.
        assert_eq!(state.locations[&LocationId(10)].clues, 2);
        assert_eq!(state.investigators[&InvestigatorId(1)].clues, 0);
        assert!(events.is_empty());
    }

    #[test]
    fn if_skill_test_kind_runs_else_branch_when_present_and_kind_differs() {
        use crate::dsl::{discover_clue, gain_resources, if_else, Condition, InvestigatorTarget};
        let mut state = state_with_in_flight_kind(SkillTestKind::Fight);
        let mut events = Vec::new();
        let effect = if_else(
            Condition::SkillTestKind(SkillTestKind::Investigate),
            discover_clue(LocationTarget::TestedLocation, 1),
            gain_resources(InvestigatorTarget::You, 2),
        );
        let resources_before = state.investigators[&InvestigatorId(1)].resources;

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        // Else branch ran: location untouched, resources +2.
        assert_eq!(state.locations[&LocationId(10)].clues, 2);
        assert_eq!(
            state.investigators[&InvestigatorId(1)].resources,
            resources_before + 2,
        );
    }

    #[test]
    fn if_skill_test_kind_rejects_without_in_flight_test() {
        use crate::dsl::{discover_clue, if_, Condition};
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let effect = if_(
            Condition::SkillTestKind(SkillTestKind::Investigate),
            discover_clue(LocationTarget::TestedLocation, 1),
        );

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(events.is_empty());
    }

    #[test]
    fn if_skill_test_outcome_condition_remains_todo() {
        // `Condition::SkillTest { outcome }` isn't yet wired. The
        // preferred path for resolution-time outcome-gated effects is
        // Trigger::OnSkillTestResolution; the condition is reserved
        // for a future past-test reaction model.
        use crate::dsl::{discover_clue, if_, Condition, TestOutcome};
        let mut state = state_with_in_flight_kind(SkillTestKind::Investigate);
        let mut events = Vec::new();
        let effect = if_(
            Condition::SkillTest {
                outcome: TestOutcome::Success,
            },
            discover_clue(LocationTarget::TestedLocation, 1),
        );

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );

        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("Condition::SkillTest"),
                    "reason should mention Condition::SkillTest: {reason:?}",
                );
            }
            _ => panic!("expected Rejected for stubbed condition, got {outcome:?}"),
        }
    }

    #[test]
    fn tested_location_rejects_when_test_has_no_location_snapshot() {
        // In-flight test exists but tested_location is None (e.g.
        // bare PerformSkillTest invoked while between locations).
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.in_flight_skill_test = Some(crate::state::InFlightSkillTest {
            investigator: InvestigatorId(1),
            skill: SkillKind::Willpower,
            kind: SkillTestKind::Plain,
            difficulty: 2,
            committed_by_active: Vec::new(),
            tested_location: None,
            follow_up: crate::state::SkillTestFollowUp::None,
            on_fail: None,
            on_success: None,
            source: None,
            continuation: crate::state::FinishContinuation::AwaitingCommit,
        });
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::TestedLocation, 1),
            ctx(1),
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(events.is_empty());
    }

    #[test]
    fn seq_runs_effects_in_order_then_done() {
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(10);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(loc_id);
        let mut location = test_location(10, "Study");
        location.clues = 1;

        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &seq([
                gain_resources(InvestigatorTarget::You, 2),
                discover_clue(LocationTarget::YourLocation, 1),
            ]),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_event!(events, Event::ResourcesGained { .. });
        assert_event!(events, Event::CluePlaced { .. });
        assert_eq!(state.investigators[&inv_id].resources, 7); // 5 default + 2
        assert_eq!(state.investigators[&inv_id].clues, 1);
    }

    #[test]
    fn seq_short_circuits_on_rejected() {
        // First effect rejects (Active without active_investigator);
        // second effect should not run.
        let loc_id = LocationId(10);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(loc_id);
        let mut location = test_location(10, "Study");
        location.clues = 1;

        let mut state = GameStateBuilder::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &seq([
                gain_resources(InvestigatorTarget::Active, 1), // rejects
                discover_clue(LocationTarget::YourLocation, 1), // shouldn't run
            ]),
            ctx(1),
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        // Location's clues should still be 1 — the discover_clue
        // never executed.
        assert_eq!(state.locations[&loc_id].clues, 1);
    }

    #[test]
    fn modify_with_while_in_play_scope_under_non_constant_trigger_rejects() {
        // WhileInPlay belongs under Trigger::Constant; reaching the
        // evaluator with this combination means the card author
        // wired the ability wrong. Reject loudly.
        let mut state = GameStateBuilder::new().build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Willpower, 1, ModifierScope::WhileInPlay),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn modify_with_this_skill_test_scope_pushes_pending_modifier() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(events.is_empty(), "push doesn't emit an event");
        assert_eq!(state.pending_skill_modifiers.len(), 1);
        let m = &state.pending_skill_modifiers[0];
        assert_eq!(m.investigator, id);
        assert_eq!(m.stat, Stat::Intellect);
        assert_eq!(m.delta, 1);
        assert_eq!(m.source, None, "no source on a bare for_controller ctx");
    }

    #[test]
    fn modify_pushes_source_when_ctx_has_one() {
        let id = InvestigatorId(1);
        let src = CardInstanceId(42);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let ctx_with_src = EvalContext::for_controller_with_source(id, src);
        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Combat, 2, ModifierScope::ThisSkillTest),
            ctx_with_src,
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.pending_skill_modifiers[0].source, Some(src));
    }

    #[test]
    fn modify_with_this_turn_scope_rejects_with_todo() {
        let mut state = GameStateBuilder::new().build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Willpower, 1, ModifierScope::ThisTurn),
            ctx(1),
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("ThisTurn"),
                    "reason should mention ThisTurn: {reason:?}",
                );
            }
            _ => panic!("expected Rejected"),
        }
    }

    #[test]
    fn choose_one_is_rejected_with_todo_message() {
        let mut state = GameStateBuilder::new().build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &Effect::ChooseOne(vec![gain_resources(InvestigatorTarget::You, 1)]),
            ctx(1),
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("ChooseOne"),
                    "reason should mention ChooseOne: {reason:?}"
                );
            }
            _ => panic!("expected Rejected"),
        }
    }

    // ---- constant-modifier query tests --------------------------

    /// Mock registry that maps a small hardcoded set of codes to
    /// abilities. Keeps the constant-modifier query tests isolated
    /// from the global `OnceLock` and from the cards crate.
    fn mock_registry(_: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
        None
    }

    fn fake_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
        match code.as_str() {
            "willpower-plus-1" => Some(vec![constant(modify(
                Stat::Willpower,
                1,
                ModifierScope::WhileInPlay,
            ))]),
            "intellect-plus-2" => Some(vec![constant(modify(
                Stat::Intellect,
                2,
                ModifierScope::WhileInPlay,
            ))]),
            "intellect-plus-1-while-investigating" => Some(vec![constant(modify(
                Stat::Intellect,
                1,
                ModifierScope::WhileInPlayDuring(SkillTestKind::Investigate),
            ))]),
            "willpower-plus-1-this-test-only" => Some(vec![constant(modify(
                Stat::Willpower,
                1,
                ModifierScope::ThisSkillTest,
            ))]),
            "willpower-minus-1" => Some(vec![constant(modify(
                Stat::Willpower,
                -1,
                ModifierScope::WhileInPlay,
            ))]),
            "non-constant-willpower" => Some(vec![on_play(modify(
                Stat::Willpower,
                5,
                ModifierScope::WhileInPlay,
            ))]),
            "max-health-plus-1" => Some(vec![constant(modify(
                Stat::MaxHealth,
                1,
                ModifierScope::WhileInPlay,
            ))]),
            "shroud-plus-2" => Some(vec![constant(modify(
                Stat::Shroud,
                2,
                ModifierScope::WhileInPlay,
            ))]),
            "cannot-play-assets" => Some(vec![constant(crate::dsl::restrict(
                crate::dsl::Restriction::CannotPlay(crate::card_data::CardType::Asset),
            ))]),
            "frozen-surcharge" => Some(vec![constant(crate::dsl::restrict(
                crate::dsl::Restriction::ExtraActionCost {
                    actions: vec![
                        crate::dsl::ActionClass::Move,
                        crate::dsl::ActionClass::Fight,
                        crate::dsl::ActionClass::Evade,
                    ],
                    first_each_round: true,
                },
            ))]),
            _ => None,
        }
    }

    fn fake_registry() -> CardRegistry {
        CardRegistry {
            metadata_for: mock_registry,
            abilities_for: fake_abilities_for,
            native_effect_for: |_| None,
        }
    }

    fn state_with_cards_in_play(codes: &[&str]) -> (crate::state::GameState, InvestigatorId) {
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.cards_in_play = codes
            .iter()
            .enumerate()
            .map(|(i, c)| {
                CardInPlay::enter_play(
                    CardCode::new(*c),
                    #[allow(clippy::cast_possible_truncation)]
                    CardInstanceId(i as u32),
                )
            })
            .collect();
        let state = GameStateBuilder::new().with_investigator(inv).build();
        (state, id)
    }

    #[test]
    fn discard_self_removes_threat_area_instance_to_encounter_discard() {
        use crate::event::Event;
        use crate::state::Zone;
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let inst = CardInstanceId(5);
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .threat_area
            .push(CardInPlay::enter_play(CardCode::new("01165"), inst));
        let mut events = Vec::new();
        let outcome = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let mut c = EvalContext::for_controller(InvestigatorId(1));
            c.source = Some(inst);
            apply_effect(&mut cx, &super::Effect::DiscardSelf, c)
        };
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.investigators[&InvestigatorId(1)]
            .threat_area
            .is_empty());
        assert_eq!(state.encounter_discard, vec![CardCode::new("01165")]);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { from: Zone::ThreatArea, code, .. } if code.as_str() == "01165"
        )));
    }

    #[test]
    fn discard_self_removes_location_attachment_to_encounter_discard() {
        use crate::event::Event;
        use crate::state::Zone;
        use crate::test_support::test_location;
        let mut loc = test_location(3, "Study");
        loc.attachments.push(CardInPlay::enter_play(
            CardCode::new("01168"),
            CardInstanceId(9),
        ));
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(loc)
            .build();
        let mut events = Vec::new();
        let outcome = {
            let mut cx = Cx {
                state: &mut state,
                events: &mut events,
            };
            let mut c = EvalContext::for_controller(InvestigatorId(1));
            c.source = Some(CardInstanceId(9));
            apply_effect(&mut cx, &super::Effect::DiscardSelf, c)
        };
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.locations[&LocationId(3)].attachments.is_empty());
        assert_eq!(state.encounter_discard, vec![CardCode::new("01168")]);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::CardDiscarded { from: Zone::LocationAttachment, code, .. } if code.as_str() == "01168"
        )));
    }

    #[test]
    fn discard_self_rejects_without_source() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = apply_effect(
            &mut cx,
            &super::Effect::DiscardSelf,
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn play_is_prohibited_matches_only_the_forbidden_type() {
        use super::play_is_prohibited;
        use crate::card_data::CardType;
        let (state, id) = state_with_cards_in_play(&["cannot-play-assets"]);
        let reg = fake_registry();
        assert!(play_is_prohibited(&state, &reg, id, CardType::Asset));
        assert!(!play_is_prohibited(&state, &reg, id, CardType::Event));
    }

    #[test]
    fn surcharge_charges_first_matching_action_then_not_again_until_reset() {
        use super::pending_action_surcharge;
        use crate::dsl::ActionClass;
        let (mut state, id) = state_with_cards_in_play(&["frozen-surcharge"]);
        let reg = fake_registry();

        // First move this round: +1, and the source (instance 0) to mark.
        let (extra, to_mark) = pending_action_surcharge(&state, &reg, id, ActionClass::Move);
        assert_eq!(extra, 1);
        assert_eq!(to_mark, vec![CardInstanceId(0)]);

        // Mark it spent (what the action handler does on commit).
        state
            .investigators
            .get_mut(&id)
            .unwrap()
            .action_surcharge_spent_this_round
            .insert(CardInstanceId(0));

        // Second matching action this round: no surcharge.
        let (extra, to_mark) = pending_action_surcharge(&state, &reg, id, ActionClass::Fight);
        assert_eq!(extra, 0);
        assert!(to_mark.is_empty());

        // New round reset → charges again.
        state
            .investigators
            .get_mut(&id)
            .unwrap()
            .action_surcharge_spent_this_round
            .clear();
        let (extra, _) = pending_action_surcharge(&state, &reg, id, ActionClass::Evade);
        assert_eq!(extra, 1);
    }

    #[test]
    fn surcharge_two_sources_each_charge_the_first_action() {
        use super::pending_action_surcharge;
        use crate::dsl::ActionClass;
        let (state, id) = state_with_cards_in_play(&["frozen-surcharge", "frozen-surcharge"]);
        let reg = fake_registry();
        let (extra, to_mark) = pending_action_surcharge(&state, &reg, id, ActionClass::Move);
        assert_eq!(
            extra, 2,
            "two Frozen in Fear each surcharge the first action"
        );
        assert_eq!(to_mark, vec![CardInstanceId(0), CardInstanceId(1)]);
    }

    #[test]
    fn play_is_prohibited_false_with_no_restriction() {
        use super::play_is_prohibited;
        use crate::card_data::CardType;
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
        let reg = fake_registry();
        assert!(!play_is_prohibited(&state, &reg, id, CardType::Asset));
    }

    #[test]
    fn effective_shroud_adds_attachment_shroud_modifiers() {
        use crate::test_support::test_location;
        let mut loc = test_location(3, "Study"); // printed shroud 2
        loc.attachments.push(CardInPlay::enter_play(
            CardCode::new("shroud-plus-2"),
            CardInstanceId(0),
        ));
        let reg = fake_registry();
        assert_eq!(effective_shroud(&reg, &loc), 4);
    }

    #[test]
    fn effective_shroud_is_printed_value_with_no_attachments() {
        use crate::test_support::test_location;
        let loc = test_location(3, "Study"); // printed shroud 2
        let reg = fake_registry();
        assert_eq!(effective_shroud(&reg, &loc), 2);
    }

    #[test]
    fn constant_modifier_is_zero_with_empty_cards_in_play() {
        let (state, id) = state_with_cards_in_play(&[]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            0
        );
    }

    #[test]
    fn unconditional_modifier_counts_while_in_play_constant() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
        let reg = fake_registry();
        assert_eq!(
            unconditional_constant_stat_modifier(&state, &reg, id, Stat::Willpower),
            1
        );
    }

    #[test]
    fn unconditional_modifier_excludes_while_in_play_during() {
        // WhileInPlayDuring needs a skill-test context; prey has none.
        let (state, id) = state_with_cards_in_play(&["intellect-plus-1-while-investigating"]);
        let reg = fake_registry();
        assert_eq!(
            unconditional_constant_stat_modifier(&state, &reg, id, Stat::Intellect),
            0
        );
    }

    #[test]
    fn unconditional_modifier_matches_exact_stat_including_max_health() {
        let (state, id) = state_with_cards_in_play(&["max-health-plus-1", "willpower-plus-1"]);
        let reg = fake_registry();
        assert_eq!(
            unconditional_constant_stat_modifier(&state, &reg, id, Stat::MaxHealth),
            1
        );
        // The willpower buff must not leak into a different stat's query.
        assert_eq!(
            unconditional_constant_stat_modifier(&state, &reg, id, Stat::Combat),
            0
        );
    }

    #[test]
    fn constant_modifier_sums_matching_skill_contributions() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1", "willpower-minus-1"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            0
        );

        let (state, id) =
            state_with_cards_in_play(&["willpower-plus-1", "willpower-plus-1", "willpower-plus-1"]);
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            3
        );
    }

    #[test]
    fn constant_modifier_ignores_non_matching_skill() {
        let (state, id) = state_with_cards_in_play(&["intellect-plus-2"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            0
        );
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Intellect, SkillTestKind::Plain),
            2
        );
    }

    #[test]
    fn constant_modifier_ignores_non_while_in_play_scope() {
        // ThisSkillTest scope shouldn't fire from constant in-play
        // query — that scope belongs to commit-time bonuses.
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1-this-test-only"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            0
        );
    }

    #[test]
    fn constant_modifier_ignores_non_constant_trigger() {
        // OnPlay-triggered Modify isn't a constant contribution; it
        // resolved once when the card was played.
        let (state, id) = state_with_cards_in_play(&["non-constant-willpower"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            0
        );
    }

    #[test]
    fn constant_modifier_ignores_non_skill_stats() {
        // MaxHealth / MaxSanity aren't skills; they should never
        // contribute to a skill-test total regardless of how the
        // helper is queried.
        let (state, id) = state_with_cards_in_play(&["max-health-plus-1"]);
        let reg = fake_registry();
        for skill in [
            SkillKind::Willpower,
            SkillKind::Intellect,
            SkillKind::Combat,
            SkillKind::Agility,
        ] {
            assert_eq!(
                constant_skill_modifier(&state, &reg, id, skill, SkillTestKind::Plain),
                0,
            );
        }
    }

    #[test]
    fn constant_modifier_skips_unknown_codes() {
        // Cards in play whose code the registry doesn't know are
        // silently skipped — the deck-import gate (Phase 9) keeps
        // unimplemented codes out of play, so silent-skip is safe.
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1", "unknown-card"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            1
        );
    }

    #[test]
    fn constant_modifier_zero_for_unknown_controller() {
        let state = GameStateBuilder::new().build();
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(
                &state,
                &reg,
                InvestigatorId(99),
                SkillKind::Willpower,
                SkillTestKind::Plain,
            ),
            0,
        );
    }

    // ---- WhileInPlayDuring scope tests ---------------------------

    #[test]
    fn while_in_play_during_contributes_only_to_matching_kind() {
        // A Magnifying-Glass-shaped card: "+1 intellect while
        // investigating." Contributes during Investigate; does NOT
        // contribute during Plain or Fight tests of intellect.
        let (state, id) = state_with_cards_in_play(&["intellect-plus-1-while-investigating"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(
                &state,
                &reg,
                id,
                SkillKind::Intellect,
                SkillTestKind::Investigate,
            ),
            1,
        );
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Intellect, SkillTestKind::Plain,),
            0,
        );
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Intellect, SkillTestKind::Fight,),
            0,
        );
    }

    #[test]
    fn while_in_play_modifier_applies_to_every_kind() {
        // Holy Rosary–shaped: unqualified `WhileInPlay`. Should
        // contribute during every test kind.
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
        let reg = fake_registry();
        for kind in [
            SkillTestKind::Investigate,
            SkillTestKind::Fight,
            SkillTestKind::Evade,
            SkillTestKind::Plain,
        ] {
            assert_eq!(
                constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, kind),
                1,
                "WhileInPlay should apply during {kind:?}",
            );
        }
    }

    #[test]
    fn while_in_play_during_with_wrong_stat_still_does_not_contribute() {
        // The intellect-while-investigating card must NOT contribute
        // to a willpower test even during Investigate. Scope and
        // stat are independent filters.
        let (state, id) = state_with_cards_in_play(&["intellect-plus-1-while-investigating"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(
                &state,
                &reg,
                id,
                SkillKind::Willpower,
                SkillTestKind::Investigate,
            ),
            0,
        );
    }

    // ---- pending_skill_modifier tests ----------------------------

    use super::pending_skill_modifier;
    use crate::state::PendingSkillModifier;

    fn state_with_pending(pending: Vec<PendingSkillModifier>) -> crate::state::GameState {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.pending_skill_modifiers = pending;
        state
    }

    #[test]
    fn pending_modifier_is_zero_with_empty_accumulator() {
        let state = state_with_pending(vec![]);
        assert_eq!(
            pending_skill_modifier(&state, InvestigatorId(1), SkillKind::Willpower),
            0,
        );
    }

    #[test]
    fn pending_modifier_sums_matching_investigator_and_stat() {
        let id = InvestigatorId(1);
        let state = state_with_pending(vec![
            PendingSkillModifier {
                investigator: id,
                stat: Stat::Intellect,
                delta: 1,
                source: None,
            },
            PendingSkillModifier {
                investigator: id,
                stat: Stat::Intellect,
                delta: 2,
                source: None,
            },
        ]);
        assert_eq!(pending_skill_modifier(&state, id, SkillKind::Intellect), 3,);
    }

    #[test]
    fn pending_modifier_ignores_other_investigators() {
        let me = InvestigatorId(1);
        let them = InvestigatorId(2);
        let state = state_with_pending(vec![PendingSkillModifier {
            investigator: them,
            stat: Stat::Willpower,
            delta: 5,
            source: None,
        }]);
        assert_eq!(pending_skill_modifier(&state, me, SkillKind::Willpower), 0,);
    }

    #[test]
    fn pending_modifier_ignores_non_matching_stat() {
        let id = InvestigatorId(1);
        let state = state_with_pending(vec![PendingSkillModifier {
            investigator: id,
            stat: Stat::Intellect,
            delta: 1,
            source: None,
        }]);
        assert_eq!(pending_skill_modifier(&state, id, SkillKind::Willpower), 0,);
    }

    #[test]
    fn pending_modifier_ignores_non_skill_stats() {
        let id = InvestigatorId(1);
        let state = state_with_pending(vec![PendingSkillModifier {
            investigator: id,
            stat: Stat::MaxHealth,
            delta: 1,
            source: None,
        }]);
        for skill in [
            SkillKind::Willpower,
            SkillKind::Intellect,
            SkillKind::Combat,
            SkillKind::Agility,
        ] {
            assert_eq!(pending_skill_modifier(&state, id, skill), 0);
        }
    }

    #[test]
    fn deal_damage_adds_damage_and_emits_event() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = apply_effect(
            &mut cx,
            &deal_damage(InvestigatorTarget::You, 2),
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].damage, 2);
        assert_event!(
            events,
            Event::DamageTaken { investigator, amount: 2 } if *investigator == InvestigatorId(1)
        );
    }

    #[test]
    fn for_each_point_failed_scales_body_by_margin() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        // Margin 2 → run DealDamage{You,1} twice → 2 damage, 2 events.
        let mut eval_ctx = EvalContext::for_controller(InvestigatorId(1));
        eval_ctx.failed_by = Some(2);
        let outcome = apply_effect(
            &mut cx,
            &for_each_point_failed(deal_damage(InvestigatorTarget::You, 1)),
            eval_ctx,
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].damage, 2);
        assert_event_count!(events, 2, Event::DamageTaken { .. });
    }

    #[test]
    fn for_each_point_failed_with_no_margin_is_a_noop() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        // failed_by None (no test in context) → zero iterations.
        let outcome = apply_effect(
            &mut cx,
            &for_each_point_failed(deal_damage(InvestigatorTarget::You, 1)),
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].damage, 0);
        assert_no_event!(events, Event::DamageTaken { .. });
    }

    #[test]
    fn deal_horror_adds_horror_and_emits_event() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = apply_effect(
            &mut cx,
            &deal_horror(InvestigatorTarget::You, 1),
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].horror, 1);
        assert_event!(
            events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
        );
    }

    #[test]
    fn deal_damage_at_max_health_defeats_investigator() {
        // Build an investigator with a known low max_health (3), then
        // apply exactly 3 damage via Effect::DealDamage and assert the
        // investigator is Killed and InvestigatorDefeated is emitted.
        use crate::state::Status;
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.max_health = 3;
        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &deal_damage(InvestigatorTarget::You, 3),
            EvalContext::for_controller(id),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].status, Status::Killed);
        assert_event!(
            events,
            Event::InvestigatorDefeated { investigator, .. } if *investigator == id
        );
    }

    #[test]
    fn advance_current_act_non_terminal_bumps_cursor() {
        use crate::scenario::Resolution;
        use crate::state::{Act, CardCode, InvestigatorId};
        use crate::test_support::GameStateBuilder;
        let mut state = GameStateBuilder::new()
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.act_deck = vec![
            Act {
                code: CardCode("a1".into()),
                clue_threshold: 0,
                resolution: None,
                round_end_advance: None,
            },
            Act {
                code: CardCode("a2".into()),
                clue_threshold: 0,
                resolution: Some(Resolution::Won { id: "R1".into() }),
                round_end_advance: None,
            },
        ];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = apply_effect(
            &mut cx,
            &Effect::AdvanceCurrentAct,
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.act_index, 1);
        assert!(state.resolution.is_none());
    }

    #[test]
    fn advance_current_act_terminal_latches_resolution() {
        use crate::scenario::Resolution;
        use crate::state::{Act, CardCode, InvestigatorId};
        use crate::test_support::GameStateBuilder;
        let mut state = GameStateBuilder::new()
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.act_deck = vec![Act {
            code: CardCode("a1".into()),
            clue_threshold: 0,
            resolution: Some(Resolution::Won { id: "R1".into() }),
            round_end_advance: None,
        }];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = apply_effect(
            &mut cx,
            &Effect::AdvanceCurrentAct,
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.act_index, 0, "terminal act does not move the cursor");
        assert!(matches!(state.resolution, Some(Resolution::Won { .. })));
    }
}
