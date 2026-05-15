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
//! - [`Effect::Modify`] is **queried**, not applied: the cards-in-play
//!   state landed with #62 and the constant-modifier query (this
//!   module's [`constant_skill_modifier`]) reads it during skill-test
//!   resolution. The `Effect::Modify` arm of [`apply_effect`] still
//!   rejects — `Modify` isn't directly *applied*; it's a passive
//!   contribution surfaced by query.
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
use crate::dsl::{Effect, InvestigatorTarget, LocationTarget, ModifierScope, Stat, Trigger};
use crate::event::Event;
use crate::state::{GameState, InvestigatorId, SkillKind};

use super::outcome::EngineOutcome;

/// Per-evaluation context the effect needs to resolve targets and
/// reference in-flight game state (current skill test, etc.).
///
/// Phase-3 minimal: just the controller's id. Grows fields as
/// effects demand them — current skill test (for
/// [`SkillTest`](crate::dsl::Condition::SkillTest) condition),
/// current target (for [`Effect::ForEach`] body), reaction-window
/// context (for `OnEvent` triggers), etc. Keep the surface narrow
/// and add fields only when an effect's evaluator actually reads
/// them.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct EvalContext {
    /// The investigator whose card-effect we're resolving — the
    /// "you" in card text. Resolves [`InvestigatorTarget::Controller`]
    /// and [`LocationTarget::ControllerLocation`].
    pub controller: crate::state::InvestigatorId,
}

impl EvalContext {
    /// Construct a context for the given controller.
    #[must_use]
    pub fn for_controller(controller: crate::state::InvestigatorId) -> Self {
        Self { controller }
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
        Effect::Modify { .. } => EngineOutcome::Rejected {
            reason: "TODO(#47): Modify evaluator needs a cards-in-play state + modifier query \
                     mechanism. Lands later in Phase 3 alongside the skill-test resolution flow."
                .into(),
        },
        Effect::If { .. } => EngineOutcome::Rejected {
            reason: "TODO(#47): If evaluator dispatches but Condition::SkillTest needs the \
                     in-flight skill test in EvalContext (lands with #49)."
                .into(),
        },
        Effect::ForEach { .. } => awaiting_input_stub("ForEach"),
        Effect::ChooseOne(_) => awaiting_input_stub("ChooseOne"),
    }
}

/// Standard rejection message for effect variants whose evaluator
/// needs `AwaitingInput` plumbing (`ChoiceResolver` / `ResolveInput`
/// resume). Centralizes the message so the un-stub path is one grep.
fn awaiting_input_stub(name: &'static str) -> EngineOutcome {
    EngineOutcome::Rejected {
        reason: format!(
            "TODO(#47): {name} evaluator needs AwaitingInput plumbing + ResolveInput resume; \
             lands with the ChoiceResolver (#19) alongside skill tests (#49)."
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
            // ResolveInput round-trip carrying the chosen id. Land
            // with the ChoiceResolver (#19) in PR-M alongside skill
            // tests.
            Err("InvestigatorTarget::ChosenByController requires AwaitingInput plumbing (PR-M)")
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
            Err("LocationTarget::ChosenByController requires AwaitingInput plumbing (PR-M)")
        }
    }
}

// ---- constant-modifier query ----------------------------------

/// Sum the constant skill-modifier contributions from every card in
/// `controller`'s `cards_in_play`.
///
/// Walks each in-play card code, looks up its abilities via the
/// supplied [`CardRegistry`], and sums every
/// [`Effect::Modify`] under a [`Trigger::Constant`] ability where the
/// scope is [`ModifierScope::WhileInPlay`] and the stat matches the
/// queried `skill`. This is the "Holy Rosary contributes +1 willpower
/// while in play" query the skill-test handler consumes.
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
/// - **Per-skill-test-kind scoping** ("while investigating" — #45):
///   only the bare stat match is checked; richer scope predicates
///   need a different `ModifierScope` variant.
/// - **Conditional constants** (`Effect::If` under a `Trigger::Constant`):
///   not yet wired; this helper ignores them.
/// - **Commit-time bonuses** (`ModifierScope::ThisSkillTest`): not in
///   scope for constants; the skill-test commit window (#63) handles
///   those.
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
) -> i8 {
    let Some(inv) = state.investigators.get(&controller) else {
        return 0;
    };
    let mut total: i8 = 0;
    for code in &inv.cards_in_play {
        let Some(abilities) = (registry.abilities_for)(code) else {
            continue;
        };
        for ability in &abilities {
            if ability.trigger != Trigger::Constant {
                continue;
            }
            let Effect::Modify { stat, delta, scope } = &ability.effect else {
                continue;
            };
            if *scope != ModifierScope::WhileInPlay {
                continue;
            }
            if stat_matches_skill(*stat, skill) {
                total = total.saturating_add(*delta);
            }
        }
    }
    total
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
        LocationTarget, ModifierScope, Stat, Trigger,
    };
    use crate::event::Event;
    use crate::state::{CardCode, InvestigatorId, LocationId, SkillKind};
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
    fn modify_is_rejected_with_todo_message() {
        let mut state = TestGame::new().build();
        let mut events = Vec::new();
        let outcome = apply_effect(
            &mut state,
            &mut events,
            &modify(Stat::Willpower, 1, ModifierScope::WhileInPlay),
            ctx(1),
        );
        match outcome {
            EngineOutcome::Rejected { reason } => {
                assert!(
                    reason.contains("Modify"),
                    "reason should mention Modify: {reason:?}"
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
        inv.cards_in_play = codes.iter().map(|c| CardCode::new(*c)).collect();
        let state = TestGame::new().with_investigator(inv).build();
        (state, id)
    }

    #[test]
    fn constant_modifier_is_zero_with_empty_cards_in_play() {
        let (state, id) = state_with_cards_in_play(&[]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower),
            0
        );
    }

    #[test]
    fn constant_modifier_sums_matching_skill_contributions() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1", "willpower-minus-1"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower),
            0
        );

        let (state, id) =
            state_with_cards_in_play(&["willpower-plus-1", "willpower-plus-1", "willpower-plus-1"]);
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower),
            3
        );
    }

    #[test]
    fn constant_modifier_ignores_non_matching_skill() {
        let (state, id) = state_with_cards_in_play(&["intellect-plus-2"]);
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower),
            0
        );
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Intellect),
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
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower),
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
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower),
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
            assert_eq!(constant_skill_modifier(&state, &reg, id, skill), 0);
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
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower),
            1
        );
    }

    #[test]
    fn constant_modifier_zero_for_unknown_controller() {
        let state = TestGame::new().build();
        let reg = fake_registry();
        assert_eq!(
            constant_skill_modifier(&state, &reg, InvestigatorId(99), SkillKind::Willpower),
            0
        );
    }
}
