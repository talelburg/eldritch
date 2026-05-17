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
//! - [`Effect::If`] dispatches but its [`Condition`](crate::dsl::Condition)
//!   evaluator is skill-test-aware
//!   ([`SkillTest`](crate::dsl::Condition::SkillTest) reads the
//!   in-flight test's outcome) and skill tests don't exist yet (#49).
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
    Effect, InvestigatorTarget, LocationTarget, ModifierScope, SkillTestKind, Stat, Trigger,
};
use crate::event::Event;
use crate::state::{GameState, InvestigatorId, SkillKind};

use super::outcome::EngineOutcome;

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
    /// "you" in card text. Resolves [`InvestigatorTarget::Controller`]
    /// and [`LocationTarget::ControllerLocation`].
    pub controller: crate::state::InvestigatorId,
    /// The in-play card-instance that triggered this effect, if any.
    /// Set by [`activate_ability`](crate::engine) so pushed
    /// [`PendingSkillModifier`](crate::state::PendingSkillModifier)
    /// entries can name their source (for replay clarity and future
    /// limit-once-per-test logic). `None` for evaluations not
    /// originating from a specific in-play instance (events played
    /// from hand, scenario forced effects, …).
    pub source: Option<crate::state::CardInstanceId>,
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
pub fn apply_effect(
    state: &mut GameState,
    events: &mut Vec<Event>,
    effect: &Effect,
    ctx: EvalContext,
) -> EngineOutcome {
    match effect {
        Effect::GainResources { target, amount } => {
            gain_resources(state, events, ctx, *target, *amount)
        }
        Effect::DiscoverClue { from, count } => discover_clue(state, events, ctx, *from, *count),
        Effect::Seq(effects) => apply_seq(state, events, effects, ctx),
        Effect::Modify { stat, delta, scope } => modify(state, ctx, *stat, *delta, *scope),
        Effect::If { .. } => EngineOutcome::Rejected {
            reason: "TODO(#47): If evaluator dispatches but Condition::SkillTest needs the \
                     in-flight skill test in EvalContext (lands with #49)."
                .into(),
        },
        Effect::ForEach { .. } => awaiting_input_stub("ForEach"),
        Effect::ChooseOne(_) => awaiting_input_stub("ChooseOne"),
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
    state: &mut GameState,
    ctx: EvalContext,
    stat: crate::dsl::Stat,
    delta: i8,
    scope: ModifierScope,
) -> EngineOutcome {
    match scope {
        ModifierScope::ThisSkillTest => {
            state
                .pending_skill_modifiers
                .push(crate::state::PendingSkillModifier {
                    investigator: ctx.controller,
                    stat,
                    delta,
                    source: ctx.source,
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
    state: &mut GameState,
    events: &mut Vec<Event>,
    ctx: EvalContext,
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
    let target_id = match resolve_investigator_target(state, ctx, target) {
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
    let Some(investigator) = state.investigators.get_mut(&target_id) else {
        return EngineOutcome::Rejected {
            reason: format!("GainResources: investigator {target_id:?} is not in the state").into(),
        };
    };
    // Saturating add: resources are u8; we don't expect cards to push
    // past 255 in practice but a saturating op is the right defensive
    // choice for a u8 counter.
    investigator.resources = investigator.resources.saturating_add(amount);
    events.push(Event::ResourcesGained {
        investigator: target_id,
        amount,
    });
    EngineOutcome::Done
}

fn discover_clue(
    state: &mut GameState,
    events: &mut Vec<Event>,
    ctx: EvalContext,
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
    let location_id = match resolve_location_target(state, ctx, from) {
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
    let Some(location) = state.locations.get(&location_id) else {
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
    // Cap the discovery at the location's actual clue count — a card
    // can't pull more clues than exist.
    let actually_taken = count.min(location.clues);
    let new_location_count = location.clues - actually_taken;

    if !state.investigators.contains_key(&ctx.controller) {
        return EngineOutcome::Rejected {
            reason: format!(
                "DiscoverClue: controller {:?} is not in the state",
                ctx.controller
            )
            .into(),
        };
    }

    // Commit the mutations. From here both writes succeed
    // unconditionally.
    state
        .locations
        .get_mut(&location_id)
        .expect("checked above")
        .clues = new_location_count;
    let investigator = state
        .investigators
        .get_mut(&ctx.controller)
        .expect("checked above");
    investigator.clues = investigator.clues.saturating_add(actually_taken);

    events.push(Event::CluePlaced {
        investigator: ctx.controller,
        count: actually_taken,
    });
    events.push(Event::LocationCluesChanged {
        location: location_id,
        new_count: new_location_count,
    });
    EngineOutcome::Done
}

fn apply_seq(
    state: &mut GameState,
    events: &mut Vec<Event>,
    effects: &[Effect],
    ctx: EvalContext,
) -> EngineOutcome {
    // Stop at the first non-Done outcome. A Rejected mid-Seq leaves
    // earlier effects committed — not great as a rollback story, but
    // matches the existing handler contract (the validate-first
    // refactor that fixes this for whole handlers is TODO'd in
    // engine/mod.rs::apply). Most card sequences are short enough
    // that the lack of mid-sequence rollback is fine for now.
    //
    // **AwaitingInput resume:** when ChooseOne et al. land and start
    // returning AwaitingInput mid-Seq, this loop will need to track
    // a resume token + remaining-effects continuation. Today
    // AwaitingInput is unreachable here (no implemented variant
    // produces it), so the simple early-return is correct for v0.
    for effect in effects {
        let outcome = apply_effect(state, events, effect, ctx);
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
/// into `EvalContext`. Use [`InvestigatorTarget::Controller`] for
/// "the player who triggered this" — it doesn't depend on phase.
fn resolve_investigator_target(
    state: &GameState,
    ctx: EvalContext,
    target: InvestigatorTarget,
) -> Result<crate::state::InvestigatorId, &'static str> {
    match target {
        InvestigatorTarget::Controller => Ok(ctx.controller),
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
        LocationTarget::ControllerLocation => state
            .investigators
            .get(&ctx.controller)
            .and_then(|i| i.current_location)
            .ok_or("LocationTarget::ControllerLocation but the controller is between locations"),
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
            if !scope_applies(*scope, kind) {
                continue;
            }
            if stat_matches_skill(*stat, skill) {
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

#[cfg(test)]
mod tests {
    use crate::card_registry::CardRegistry;
    use crate::dsl::{
        constant, discover_clue, gain_resources, modify, seq, Ability, Effect, InvestigatorTarget,
        LocationTarget, ModifierScope, SkillTestKind, Stat, Trigger,
    };
    use crate::event::Event;
    use crate::state::{
        CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, SkillKind,
    };
    use crate::test_support::{test_investigator, test_location, TestGame};
    use crate::{assert_event, assert_no_event};

    use super::{apply_effect, constant_skill_modifier, EngineOutcome, EvalContext};

    fn ctx(id: u32) -> EvalContext {
        EvalContext::for_controller(InvestigatorId(id))
    }

    #[test]
    fn gain_resources_increments_target_wallet_and_emits_event() {
        let id = InvestigatorId(1);
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let resources_before = state.investigators[&id].resources;
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &gain_resources(InvestigatorTarget::Controller, 3),
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
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let resources_before = state.investigators[&id].resources;
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
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

        let mut state = TestGame::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &discover_clue(LocationTarget::ControllerLocation, 1),
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
    fn discover_clue_caps_at_location_clue_count() {
        // Card asks for 3 clues but the location only has 1 — take
        // what's there, no error.
        let loc_id = LocationId(10);
        let mut investigator = test_investigator(1);
        investigator.current_location = Some(loc_id);
        let mut location = test_location(10, "Study");
        location.clues = 1;

        let mut state = TestGame::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &discover_clue(LocationTarget::ControllerLocation, 3),
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

        let mut state = TestGame::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &discover_clue(LocationTarget::ControllerLocation, 1),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.locations[&loc_id].clues, 0);
        assert_eq!(state.investigators[&InvestigatorId(1)].clues, 0);
        assert_no_event!(events, Event::CluePlaced { .. });
    }

    #[test]
    fn discover_clue_rejects_when_controller_is_between_locations() {
        // Controller has no current_location — LocationTarget::
        // ControllerLocation can't resolve.
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1)) // current_location = None
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &discover_clue(LocationTarget::ControllerLocation, 1),
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

        let mut state = TestGame::new()
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
        });
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
            .with_investigator(investigator)
            .with_location({
                let mut l = test_location(10, "Study");
                l.clues = 1;
                l
            })
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &discover_clue(LocationTarget::TestedLocation, 1),
            ctx(1),
        );

        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(events.is_empty());
    }

    #[test]
    fn tested_location_rejects_when_test_has_no_location_snapshot() {
        // In-flight test exists but tested_location is None (e.g.
        // bare PerformSkillTest invoked while between locations).
        let mut state = TestGame::new()
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
        });
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
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

        let mut state = TestGame::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &seq([
                gain_resources(InvestigatorTarget::Controller, 2),
                discover_clue(LocationTarget::ControllerLocation, 1),
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

        let mut state = TestGame::new()
            .with_investigator(investigator)
            .with_location(location)
            .build();
        let mut events = Vec::new();

        let outcome = apply_effect(
            &mut state,
            &mut events,
            &seq([
                gain_resources(InvestigatorTarget::Active, 1), // rejects
                discover_clue(LocationTarget::ControllerLocation, 1), // shouldn't run
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
        let mut state = TestGame::new().build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut state,
            &mut events,
            &modify(Stat::Willpower, 1, ModifierScope::WhileInPlay),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn modify_with_this_skill_test_scope_pushes_pending_modifier() {
        let id = InvestigatorId(1);
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let ctx_with_src = EvalContext::for_controller_with_source(id, src);
        let outcome = apply_effect(
            &mut state,
            &mut events,
            &modify(Stat::Combat, 2, ModifierScope::ThisSkillTest),
            ctx_with_src,
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.pending_skill_modifiers[0].source, Some(src));
    }

    #[test]
    fn modify_with_this_turn_scope_rejects_with_todo() {
        let mut state = TestGame::new().build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut state,
            &mut events,
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
        let mut state = TestGame::new().build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut state,
            &mut events,
            &Effect::ChooseOne(vec![gain_resources(InvestigatorTarget::Controller, 1)]),
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
            "non-constant-willpower" => Some(vec![Ability {
                trigger: Trigger::OnPlay,
                costs: Vec::new(),
                effect: modify(Stat::Willpower, 5, ModifierScope::WhileInPlay),
            }]),
            "max-health-plus-1" => Some(vec![constant(modify(
                Stat::MaxHealth,
                1,
                ModifierScope::WhileInPlay,
            ))]),
            _ => None,
        }
    }

    fn fake_registry() -> CardRegistry {
        CardRegistry {
            metadata_for: mock_registry,
            abilities_for: fake_abilities_for,
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
        let state = TestGame::new().with_investigator(inv).build();
        (state, id)
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
        let state = TestGame::new().build();
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
        let mut state = TestGame::new()
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
}
