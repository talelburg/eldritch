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
//!   [`WhileInPlayDuring`] contributions are passive and swept
//!   from card abilities directly by
//!   [`modified_value`](crate::engine::modified_value::modified_value) —
//!   reaching `apply_effect` with one of those means the card
//!   author put a constant-flavored modifier under a non-constant
//!   trigger, which rejects. [`ThisSkillTest`] is **recorded** into
//!   [`GameState::recorded_modifiers`], which the same query reads as a
//!   recorded row — stamped with the in-flight test's id, and refused
//!   outright when no test is in flight; the skill-test handler expires
//!   it after `SkillTestEnded`. [`ThisTurn`] is not yet wired; rejects
//!   with TODO until a card or test demands the turn-scoped
//!   accumulator.
//! - [`Effect::AutoResolve`] writes a test's
//!   [`Determination`] into the same recorded
//!   population, under the same gate: with no test in flight there is no
//!   identity to stamp, so it rejects rather than banking a determination
//!   for whatever test comes next.
//!
//! [`WhileInPlay`]: crate::dsl::ModifierScope::WhileInPlay
//! [`WhileInPlayDuring`]: crate::dsl::ModifierScope::WhileInPlayDuring
//! [`ThisSkillTest`]: crate::dsl::ModifierScope::ThisSkillTest
//! [`ThisTurn`]: crate::dsl::ModifierScope::ThisTurn
//! [`GameState::recorded_modifiers`]: crate::state::GameState::recorded_modifiers
//! - [`Effect::If`] evaluates [`Condition::SkillTestKind`] against
//!   the in-flight test's `kind`. [`SkillTest`](crate::dsl::Condition::SkillTest)
//!   isn't yet wired — inside an [`Trigger::OnSkillTestResolution`] effect
//!   the trigger itself gates outcome, so the condition is redundant there,
//!   and no other trigger has yet needed it (the outcome *is* on
//!   [`InFlightSkillTest::resolved`](crate::state::InFlightSkillTest::resolved)
//!   from ST.6 on, so this is unwired, not unknowable).
//! - [`Effect::ForEach`] dispatches but the
//!   [`InvestigatorTargetSet`](crate::dsl::InvestigatorTargetSet)
//!   resolver ("at controller location", "all investigators")
//!   relies on per-target context that's not yet wired through.
//! - [`Effect::ChooseOne`] and the `*::Chosen` targets resolve
//!   interactively via the frame-driven choice machinery (`step_choose_one` /
//!   `ground_chosen_targets`): each auto-binds 0/1 options and suspends on 2+ by
//!   leaving the node's own [`EffectFrame::Leaf`](crate::state::EffectFrame::Leaf)
//!   on the stack as the prompt; resume sets `chosen_option` and re-steps it (no
//!   replay — #422). A `Chosen` target
//!   honors its scope: `Anywhere` offers all investigators / locations,
//!   `EntityScope::At(Here)` / `LocationSet::Here` filters to the chooser's
//!   location (#349). The enemy variety and `YourOrConnecting` are deferred to
//!   their consuming PRs (#301 / #306).
//!
//! # State-mutation contract
//!
//! `apply_effect` follows the same validate-first / mutate-second
//! pattern the existing dispatch handlers use: if the effect can't
//! resolve cleanly, return [`EngineOutcome::Rejected`]. Partial
//! mutation before a mid-tree rejection is fully rolled back at the
//! apply boundary — `apply_via` (`engine/mod.rs`) snapshot-restores
//! state, events, and RNG position on `Rejected` — so validate-first
//! here is about cheap, precise rejections, not state safety.

use serde::{Deserialize, Serialize};

use crate::card_registry::CardRegistry;
use crate::dsl::{
    Ability, CmpOp, Condition, ControlStatus, Determination, Effect, EnemyTarget, HarmKind,
    IntExpr, InvestigatorTarget, LocationTarget, ModifierScope, Quantity, Trigger,
};
use crate::event::Event;
use crate::state::{CandidateSource, GameState, InvestigatorId};

use super::outcome::EngineOutcome;
use super::Cx;

/// Failure margin of the just-resolved skill test (bound only while running an
/// `on_fail` effect). Innermost-only: same-kind test nesting is carried by the
/// per-frame snapshot stack, not multiple slots here (corpus-verified moot — no
/// card reads a non-innermost margin; see the §1 cleanup spec §D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTestBinding {
    /// Points the test was failed by.
    pub failed_by: u8,
}

/// The clue count of the discovery an ability is responding to (bound only
/// while resolving a `DiscoverClues` ability's effect, in any of the three
/// cells — a `when` replacement reads it to know how many clues it is
/// replacing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryBinding {
    /// Clues the discovery moves — what a `when` replacement is replacing.
    pub clue_discovery_count: u8,
}

/// Attacking enemy bound while resolving a `DamageAssigned` reaction whose
/// source is an enemy attack (Guard Dog 01021's retaliate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnemyAttackBinding {
    /// The enemy whose attack is being reacted to.
    pub attacking_enemy: crate::state::EnemyId,
}

/// Controller picks bound while grounding `*::Chosen` targets. Cohesive: the
/// four `*::Chosen` kinds compose on one binding (a single effect may pick an
/// investigator *and* a location). `Default` is all-`None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChoiceBinding {
    /// `InvestigatorTarget::Chosen` pick.
    pub investigator: Option<crate::state::InvestigatorId>,
    /// `LocationTarget::Chosen` pick.
    pub location: Option<crate::state::LocationId>,
    /// `EnemyTarget::Chosen` pick.
    pub enemy: Option<crate::state::EnemyId>,
    /// Native-leaf option pick.
    pub option: Option<crate::engine::OptionId>,
}

/// Per-evaluation context the effect needs to resolve targets and
/// reference in-flight game state (current skill test, etc.).
///
/// Phase-3 minimal. Grows fields as effects demand them — current
/// skill test (for [`SkillTest`](crate::dsl::Condition::SkillTest)
/// condition), current target (for [`Effect::ForEach`] body),
/// reaction-window context (for `OnEvent` triggers), etc. Keep the
/// surface narrow and add fields only when an effect's evaluator
/// actually reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EvalContext {
    /// The investigator whose card-effect we're resolving — the
    /// "you" in card text. Resolves [`InvestigatorTarget::You`]
    /// and [`LocationTarget::YourLocation`].
    pub controller: crate::state::InvestigatorId,
    /// The in-play card-instance that triggered this effect, if any.
    /// Set by [`activate_ability`](crate::engine) so recorded
    /// [`RecordedModifier`](crate::state::RecordedModifier)
    /// rows can name their source (for replay clarity and future
    /// limit-once-per-test logic). `None` for evaluations not
    /// originating from a specific in-play instance (events played
    /// from hand, scenario forced effects, …).
    pub source: Option<crate::state::CardInstanceId>,
    /// Skill-test margin binding, bound only while running an `on_fail` effect.
    /// Read via [`Self::failed_by`]. `None` outside that window.
    pub skill_test: Option<SkillTestBinding>,
    /// Clue-discovery binding, bound only while resolving a `DiscoverClues`
    /// ability's effect (any cell). Read via [`Self::clue_discovery_count`].
    /// `None` outside that window.
    pub discovery: Option<DiscoveryBinding>,
    /// Enemy-attack reaction binding, bound only while resolving an
    /// enemy-attack `DamageAssigned` reaction. Read via [`Self::attacking_enemy`].
    /// `None` outside that window. (C5b #237.)
    pub enemy_attack: Option<EnemyAttackBinding>,
    /// Grounded `*::Chosen` picks, bound during a grounded-choice evaluation
    /// (Axis A #334). Read via [`Self::chosen_investigator`] /
    /// [`Self::chosen_location`] / [`Self::chosen_enemy`] /
    /// [`Self::chosen_option`]. `None` outside a grounded choice.
    pub choice: Option<ChoiceBinding>,
    /// The ability source this effect is *printed on*, when the dispatch site
    /// knows it — the board home an effect-internal [`Effect::ChooseOne`]
    /// anchors its options to (#555). Read via [`Self::ability_source`].
    ///
    /// Strictly wider than [`source`](Self::source), which is this value's
    /// [`AbilitySource::instance`](crate::state::AbilitySource::instance)
    /// projection and is therefore `None` for exactly the board sources — the
    /// act, the agenda, a location, an enemy — that had no anchor to begin
    /// with. The two coexist rather than collapsing because 17 of the 19
    /// construction sites hold a bare `CardInstanceId` and deciding *which*
    /// `AbilitySource` each one is does not fail to compile when wrong; see
    /// #834.
    pub ability_source: Option<crate::state::AbilitySource>,
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
            skill_test: None,
            discovery: None,
            enemy_attack: None,
            choice: None,
            ability_source: None,
        }
    }

    /// Construct a context for an effect triggered from a specific
    /// in-play card instance. Used by
    /// [`activate_ability`](crate::engine) so recorded
    /// `RecordedModifier`s carry their source.
    #[must_use]
    pub fn for_controller_with_source(
        controller: crate::state::InvestigatorId,
        source: crate::state::CardInstanceId,
    ) -> Self {
        Self {
            controller,
            source: Some(source),
            skill_test: None,
            discovery: None,
            enemy_attack: None,
            choice: None,
            ability_source: None,
        }
    }

    /// Construct a context for `controller`, threading `source` when present.
    /// The common shape where a candidate / pending suspension carries an
    /// *optional* firing instance (in-play reaction or weapon ⇒ `Some`;
    /// scenario board card or hand-played event ⇒ `None`). Collapses the
    /// `match source { Some => with_source, None => for_controller }` repeated
    /// at the skill-test, choice-resume, forced-run, and reaction-window
    /// dispatch sites. Pair with
    /// [`CandidateSource::instance`](crate::state::CandidateSource::instance)
    /// when the source is a `CandidateSource`.
    #[must_use]
    pub fn for_controller_with_optional_source(
        controller: crate::state::InvestigatorId,
        source: Option<crate::state::CardInstanceId>,
    ) -> Self {
        match source {
            Some(src) => Self::for_controller_with_source(controller, src),
            None => Self::for_controller(controller),
        }
    }

    /// Record the [`AbilitySource`](crate::state::AbilitySource) whose ability
    /// is being resolved (see [`ability_source`](Self::ability_source)). Set by
    /// the forced run and the reaction window, the two dispatch sites that hold
    /// a `CandidateSource`; every other site leaves it `None` and its choices
    /// stay un-anchored, exactly as before #555.
    #[must_use]
    pub fn with_ability_source(mut self, source: Option<crate::state::AbilitySource>) -> Self {
        self.ability_source = source;
        self
    }
}

impl EvalContext {
    /// Just-resolved skill test's failure margin (bound only while running an
    /// `on_fail` effect). Consumed by `IntExpr::Count(Quantity::SkillTestFailedBy)`.
    #[must_use]
    pub fn failed_by(&self) -> Option<u8> {
        self.skill_test.map(|b| b.failed_by)
    }
    /// The clue count of the discovery this ability is responding to — what a
    /// `when` replacement is replacing (Cover Up 01007's "that many"). Bound
    /// only while resolving a `DiscoverClues` ability's effect.
    #[must_use]
    pub fn clue_discovery_count(&self) -> Option<u8> {
        self.discovery.map(|b| b.clue_discovery_count)
    }
    /// Attacking enemy bound while resolving an enemy-attack `DamageAssigned`
    /// reaction (Guard Dog 01021's retaliate).
    #[must_use]
    pub fn attacking_enemy(&self) -> Option<crate::state::EnemyId> {
        self.enemy_attack.map(|b| b.attacking_enemy)
    }
    /// Investigator picked for an `InvestigatorTarget::Chosen`.
    #[must_use]
    pub fn chosen_investigator(&self) -> Option<crate::state::InvestigatorId> {
        self.choice.and_then(|c| c.investigator)
    }
    /// Location picked for a `LocationTarget::Chosen`.
    #[must_use]
    pub fn chosen_location(&self) -> Option<crate::state::LocationId> {
        self.choice.and_then(|c| c.location)
    }
    /// Enemy picked for an `EnemyTarget::Chosen`.
    #[must_use]
    pub fn chosen_enemy(&self) -> Option<crate::state::EnemyId> {
        self.choice.and_then(|c| c.enemy)
    }
    /// Option picked for a native leaf that suspended for a choice.
    #[must_use]
    pub fn chosen_option(&self) -> Option<crate::engine::OptionId> {
        self.choice.and_then(|c| c.option)
    }
    /// The ability source this effect is printed on, if the dispatch site knew
    /// it (see [`Self::ability_source`]).
    #[must_use]
    pub fn ability_source(&self) -> Option<crate::state::AbilitySource> {
        self.ability_source
    }

    /// Bind the skill-test failure margin (see [`Self::failed_by`]).
    pub fn set_failed_by(&mut self, margin: u8) {
        self.skill_test = Some(SkillTestBinding { failed_by: margin });
    }
    /// Bind the discovery's clue count (see [`Self::clue_discovery_count`]).
    pub fn set_clue_discovery_count(&mut self, count: u8) {
        self.discovery = Some(DiscoveryBinding {
            clue_discovery_count: count,
        });
    }
    /// Bind the attacking enemy (see [`Self::attacking_enemy`]).
    pub fn set_attacking_enemy(&mut self, enemy: crate::state::EnemyId) {
        self.enemy_attack = Some(EnemyAttackBinding {
            attacking_enemy: enemy,
        });
    }
    /// Bind the chosen investigator (see [`Self::chosen_investigator`]).
    pub fn set_chosen_investigator(&mut self, id: crate::state::InvestigatorId) {
        self.choice
            .get_or_insert_with(Default::default)
            .investigator = Some(id);
    }
    /// Bind the chosen location (see [`Self::chosen_location`]).
    pub fn set_chosen_location(&mut self, id: crate::state::LocationId) {
        self.choice.get_or_insert_with(Default::default).location = Some(id);
    }
    /// Bind the chosen enemy (see [`Self::chosen_enemy`]).
    pub fn set_chosen_enemy(&mut self, id: crate::state::EnemyId) {
        self.choice.get_or_insert_with(Default::default).enemy = Some(id);
    }
    /// Bind (or clear) the native-leaf chosen option (see [`Self::chosen_option`]).
    pub fn set_chosen_option(&mut self, opt: Option<crate::engine::OptionId>) {
        // Match the old flat-field semantics exactly: a `None` pick must NOT
        // materialize an otherwise-empty `choice` binding (which would make
        // `EvalContext` compare unequal to a never-touched one). Only create the
        // binding to store a `Some`; otherwise clear an existing slot in place.
        match opt {
            Some(_) => self.choice.get_or_insert_with(Default::default).option = opt,
            None => {
                if let Some(choice) = self.choice.as_mut() {
                    choice.option = None;
                }
            }
        }
    }
}

/// Push an effect's root [`EffectFrame`](crate::state::EffectFrame) onto the
/// continuation stack for the global `drive` loop to own (top-frame dispatch,
/// #393/#423). The caller returns [`EngineOutcome::Done`]; `drive` then steps
/// the pushed frame via its [`Continuation::Effect`](crate::state::Continuation::Effect)
/// arm. Replaced the synchronous `apply_effect` bounded entry at every
/// production site (Slice D #423 retired that wrapper).
pub(crate) fn push_effect(cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext) {
    cx.state
        .continuations
        .push(crate::state::Continuation::Effect(frame_of(
            effect, eval_ctx,
        )));
}

/// Build the [`EffectFrame`](crate::state::EffectFrame) for an effect node:
/// control nodes get their own stateful frame; everything else (leaves, `If`,
/// `ChooseOne`, `SearchDeck`, `Native`) is a `Leaf` evaluated by [`step_leaf`].
fn frame_of(effect: &Effect, ctx: EvalContext) -> crate::state::EffectFrame {
    use crate::state::EffectFrame;
    match effect {
        Effect::Seq(effects) => EffectFrame::Seq {
            effects: effects.clone(),
            next: 0,
            ctx,
        },
        _ => EffectFrame::Leaf {
            effect: Box::new(effect.clone()),
            ctx,
        },
    }
}

/// Step the top [`Continuation::Effect`](crate::state::Continuation::Effect)
/// frame once: advance a `Seq`/loop cursor (pushing the next child), or evaluate
/// a `Leaf` (running it, pushing a chosen branch, or suspending in place for a
/// pick). Pops completed frames. Driven by the global `drive` loop's
/// `Continuation::Effect` arm (for effect frames parked across an `apply()`
/// boundary).
pub(crate) fn step_effect_frame(cx: &mut Cx) -> EngineOutcome {
    use crate::state::{Continuation, EffectFrame};
    let Some(Continuation::Effect(frame)) = cx.state.continuations.pop() else {
        unreachable!("step_effect_frame: top frame is not a Continuation::Effect");
    };
    match frame {
        EffectFrame::Seq { effects, next, ctx } => {
            if next < effects.len() {
                let child = frame_of(&effects[next], ctx);
                cx.state
                    .continuations
                    .push(Continuation::Effect(EffectFrame::Seq {
                        effects,
                        next: next + 1,
                        ctx,
                    }));
                cx.state.continuations.push(Continuation::Effect(child));
            }
            EngineOutcome::Done
        }
        EffectFrame::Leaf { effect, ctx } => step_leaf(cx, &effect, ctx),
        EffectFrame::Designated { designator, ctx } => step_designated(cx, &designator, ctx),
    }
}

/// Push a [`EffectFrame::Designated`](crate::state::EffectFrame::Designated)
/// frame for the global `drive` loop to own — the designated action of an
/// activated ability (#805). The [`push_effect`] twin for the half of an
/// ability that is not an effect.
pub(crate) fn push_designated_action(
    cx: &mut Cx,
    designator: &crate::dsl::ActionDesignator,
    eval_ctx: EvalContext,
) {
    cx.state
        .continuations
        .push(crate::state::Continuation::Effect(
            crate::state::EffectFrame::Designated {
                designator: Box::new(designator.clone()),
                ctx: eval_ctx,
            },
        ));
}

/// Step a [`EffectFrame::Designated`](crate::state::EffectFrame::Designated)
/// frame: ground the target the designated action needs, then perform it.
///
/// A **Fight** whose target is not yet bound grounds against the co-located
/// enemy list — auto on exactly one, suspend-in-place on 2+ (re-pushing this
/// frame, which *is* the prompt, exactly as [`step_leaf`] does). Everything
/// else needs no grounding and performs immediately.
fn step_designated(
    cx: &mut Cx,
    designator: &crate::dsl::ActionDesignator,
    eval_ctx: EvalContext,
) -> EngineOutcome {
    let eval_ctx = if matches!(designator, crate::dsl::ActionDesignator::Fight { .. })
        && eval_ctx.chosen_enemy().is_none()
    {
        match ground_fight_target_choice(cx, eval_ctx) {
            Ok(ctx) => ctx,
            Err(outcome) => {
                if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                    push_designated_action(cx, designator, eval_ctx);
                }
                return outcome;
            }
        }
    } else {
        eval_ctx
    };
    perform_designated(cx, designator, &eval_ctx)
}

/// Perform the action a bold [`ActionDesignator`](crate::dsl::ActionDesignator)
/// names, modified in the manner the ability carries (#805).
/// `glossary/Ability.md`, verbatim:
///
/// > Some abilities have bold action designators (such as **Fight**, **Evade**,
/// > **Investigate**, or **Move**). Activating such an ability **performs the
/// > designated action** as described in the rules, but modified in the manner
/// > described by the ability.
///
/// Every arm routes through the **same primary the basic action uses**
/// (`actions::perform_fight` / `actions::perform_investigate` /
/// `elimination::resign_investigator`), passing the modification as its only
/// difference — so *"a designated Fight is a Fight action"* holds in code rather
/// than by parallel construction.
///
/// The eligibility every arm assumes was checked pre-cost by
/// [`can_perform`](crate::engine::designator::can_perform); the rejections here
/// are the state-shape residue of a board that changed underneath (an attack of
/// opportunity moved the actor), and `apply_via` rolls the whole activation back
/// with them.
fn perform_designated(
    cx: &mut Cx,
    designator: &crate::dsl::ActionDesignator,
    eval_ctx: &EvalContext,
) -> EngineOutcome {
    use crate::dsl::ActionDesignator as D;
    match designator {
        D::Fight {
            combat_modifier,
            extra_damage,
        } => perform_designated_fight(cx, eval_ctx, combat_modifier, extra_damage),
        D::Investigate { shroud_modifier } => {
            let Some(location_id) =
                crate::engine::designator::investigate_location(cx.state, eval_ctx.controller)
            else {
                return EngineOutcome::Rejected {
                    reason: "Investigate: no revealed location to investigate".into(),
                };
            };
            crate::engine::dispatch::actions::perform_investigate(
                cx,
                eval_ctx.controller,
                location_id,
                Some(shroud_modifier.clone()),
                eval_ctx.source,
            )
        }
        D::Resign => {
            crate::engine::dispatch::elimination::resign_investigator(cx, eval_ctx.controller);
            EngineOutcome::Done
        }
        // `glossary/Parley.md` in full: *"Some abilities are identified with a
        // **Parley** action designator. Such abilities are initiated using the
        // 'Activate' action."* No procedure, so nothing to perform — the
        // ability's whole content is its residual effect.
        D::Parley => EngineOutcome::Done,
        // Unreachable through the activation path: `can_perform` rejects both
        // pre-cost, since no implemented card prints either (`TODO(#818)`).
        // Shares that rejection's wording so the two cannot drift.
        D::Evade | D::Move => EngineOutcome::Rejected {
            reason: crate::engine::designator::unimplemented_designator(designator),
        },
    }
}

/// The **Fight** arm of [`perform_designated`]: evaluate `extra_damage` against
/// the live board, then run the shared Fight primary against the grounded
/// target.
///
/// `extra_damage` is evaluated here because the Fight follow-up consumes it as a
/// `u8`; `combat_modifier` is **not**, travelling into the test as an
/// unevaluated [`IntExpr`] so the row it becomes is recalculated at every read
/// (ADR 0005). Machete 01020's conditional `+1` is why the evaluation happens
/// on this context: `sole_engaged_target` reads the chosen enemy the step above
/// just bound.
fn perform_designated_fight(
    cx: &mut Cx,
    eval_ctx: &EvalContext,
    combat_modifier: &IntExpr,
    extra_damage: &IntExpr,
) -> EngineOutcome {
    let extra_damage_n = match eval_int_expr(cx.state, eval_ctx, extra_damage) {
        Ok(v) => u8::try_from(v.max(0)).unwrap_or(u8::MAX),
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    // Bound by `step_designated` before this runs, and `can_perform` rejected
    // an empty candidate list pre-cost, so `None` is a state-shape violation.
    let Some(enemy_id) = eval_ctx.chosen_enemy() else {
        return EngineOutcome::Rejected {
            reason: "Fight: no co-located enemy chosen".into(),
        };
    };
    // `enemy_id` came from `enemies_in_scope` over this same map, so it is
    // present; its absence is a state-corruption invariant violation.
    assert!(
        cx.state.enemies.contains_key(&enemy_id),
        "Fight chosen_enemy returned an id absent from state.enemies",
    );
    crate::engine::dispatch::actions::perform_fight(
        cx,
        eval_ctx.controller,
        enemy_id,
        Some(combat_modifier.clone()),
        extra_damage_n,
        eval_ctx.source,
    )
}

/// Re-push a suspended `Leaf` so resume re-steps it with `ctx.chosen_option`
/// set, and return the `AwaitingInput` prompt. The frame *is* the prompt.
fn suspend_leaf_in_place(cx: &mut Cx, effect: &Effect, ctx: EvalContext) {
    cx.state
        .continuations
        .push(crate::state::Continuation::Effect(
            crate::state::EffectFrame::Leaf {
                effect: Box::new(effect.clone()),
                ctx,
            },
        ));
}

/// Evaluate one non-control effect node (the [`EffectFrame::Leaf`](crate::state::EffectFrame::Leaf)
/// step). Grounds any `*::Chosen` target, then dispatches: a terminal effect
/// runs; `If` pushes its chosen branch; `ChooseOne`/`SearchDeck`/`Native` push a
/// branch or **suspend in place** (re-pushing this `Leaf` so resume re-steps it
/// with `ctx.chosen_option` set). Control nodes (`Seq`) are normally routed to
/// their own frames by [`frame_of`]; if one reaches here it is pushed as its
/// frame. The `Leaf` itself was already popped by the caller.
// A single exhaustive dispatch over every `Effect` variant; splitting it would
// only obscure the dispatch (it mirrors the former `apply_effect_inner`).
#[allow(clippy::too_many_lines)]
fn step_leaf(cx: &mut Cx, effect: &Effect, eval_ctx: EvalContext) -> EngineOutcome {
    // Ground any `Chosen` target this node carries before running it. On a 2+
    // candidate suspension, re-push this Leaf (the prompt) and surface the
    // AwaitingInput; resume re-steps with `chosen_option` set (#422).
    let eval_ctx = match ground_chosen_targets(cx, effect, eval_ctx) {
        Ok(ctx) => ctx,
        Err(outcome) => {
            if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
                suspend_leaf_in_place(cx, effect, eval_ctx);
            }
            return outcome;
        }
    };
    match effect {
        Effect::GainResources { target, amount } => gain_resources(cx, eval_ctx, *target, *amount),
        Effect::DiscoverClue { from, count } => discover_clue(cx, eval_ctx, *from, *count),
        Effect::Deal {
            kind,
            target,
            amount,
        } => {
            let n = match eval_int_expr(cx.state, &eval_ctx, amount) {
                Ok(v) => u8::try_from(v.max(0)).unwrap_or(u8::MAX),
                Err(reason) => {
                    return EngineOutcome::Rejected {
                        reason: reason.into(),
                    }
                }
            };
            deal_effect(cx, eval_ctx, *kind, *target, n)
        }
        Effect::DealDamageToEnemy { target, amount } => {
            deal_damage_to_enemy_effect(cx, eval_ctx, *target, *amount)
        }
        Effect::Heal {
            kind,
            target,
            count,
        } => heal_effect(cx, eval_ctx, *kind, *target, *count),
        Effect::Seq(_) => {
            cx.state
                .continuations
                .push(crate::state::Continuation::Effect(frame_of(
                    effect, eval_ctx,
                )));
            EngineOutcome::Done
        }
        Effect::Modify {
            stat,
            delta,
            scope,
            audience,
        } => modify(cx, eval_ctx, *stat, *delta, *scope, *audience),
        Effect::AutoResolve { determination } => auto_resolve(cx, eval_ctx, *determination),
        Effect::If {
            condition,
            then,
            else_,
        } => {
            let holds = match eval_condition(cx.state, &eval_ctx, condition) {
                Ok(b) => b,
                Err(reason) => {
                    return EngineOutcome::Rejected {
                        reason: reason.into(),
                    }
                }
            };
            if holds {
                cx.state
                    .continuations
                    .push(crate::state::Continuation::Effect(frame_of(then, eval_ctx)));
            } else if let Some(else_branch) = else_ {
                cx.state
                    .continuations
                    .push(crate::state::Continuation::Effect(frame_of(
                        else_branch,
                        eval_ctx,
                    )));
            }
            EngineOutcome::Done
        }
        Effect::ForEach { .. } => awaiting_input_stub("ForEach"),
        Effect::ChooseOne(branches) => step_choose_one(cx, branches, eval_ctx, effect),
        Effect::AdvanceCurrentAct => apply_advance_current_act(cx),
        Effect::ReachResolution(n) => apply_reach_resolution(cx, *n),
        Effect::PlaceDoomOnCurrentAgenda { count, may_advance } => {
            apply_place_doom_on_current_agenda(cx, &eval_ctx, count, *may_advance)
        }
        Effect::Native { tag } => step_native(cx, tag, eval_ctx, effect),
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
            // A Revelation skill test takes its difficulty as printed: the
            // number is a base value on the card, not a snapshot of
            // anything on the board.
            crate::state::DifficultyBasis::Fixed(i8::try_from(*difficulty).unwrap_or(i8::MAX)),
            crate::state::SkillTestFollowUp::None,
            on_success.as_ref().map(|b| (**b).clone()),
            on_fail.as_ref().map(|b| (**b).clone()),
            eval_ctx.source,
            None,
        ),
        Effect::DiscardSelf => discard_self(cx, &eval_ctx),
        Effect::Cancel => cancel_current_impact(cx),
        Effect::PutIntoThreatArea { code, clues } => {
            let inst = crate::engine::dispatch::threat_area::place_in_threat_area(
                cx,
                eval_ctx.controller,
                crate::state::CardCode::new(code.clone()),
            );
            let placed = inst.and_then(|id| {
                cx.state
                    .investigators
                    .get_mut(&eval_ctx.controller)
                    .and_then(|inv| inv.threat_area.iter_mut().find(|c| c.instance_id == id))
            });
            if let Some(card) = placed {
                card.clues = *clues;
            }
            EngineOutcome::Done
        }
        Effect::Restrict(_) => EngineOutcome::Rejected {
            reason: "Effect::Restrict is a constant marker — inspected at decision points, \
                     never executed"
                .into(),
        },
        Effect::Grant { .. } => EngineOutcome::Rejected {
            reason: "Effect::Grant is a constant marker — swept off the board by \
                     engine::abilities_in_effect, never executed"
                .into(),
        },
        Effect::TakeControl { code } => {
            crate::engine::dispatch::take_control(cx, eval_ctx.controller, code)
        }
        Effect::BoostAttackDamage(amount) => boost_attack_damage_effect(cx, *amount),
        Effect::DiscoverAdditionalClues(amount) => discover_additional_clues_effect(cx, *amount),
        Effect::DrawCards { target, count } => draw_cards_effect(cx, eval_ctx, *target, *count),
        Effect::SearchDeck {
            target,
            scope,
            filter,
        } => apply_search_deck(cx, eval_ctx, *target, *scope, filter.as_ref(), effect),
        Effect::AttachSelfToLocation => apply_attach_self_to_location(cx),
    }
}

/// The [`Effect::ChooseOne`] step: auto-resolve / pick the branch (re-stepped
/// with `ctx.chosen_option` after a resume), or **suspend in place** by
/// re-pushing `node` as a `Leaf` and returning the prompt. No replay.
///
/// The offered branches are the **live** ones — those
/// [`effect_can_change_state`] cannot prove inert (#664). RR "Ability" reads
/// per mode: *"A triggered ability can only be initiated if its effect has the
/// potential to change the game state"*, and RR "Target" makes a target with
/// nothing to change ineligible, so First Aid 01019's *"Heal 1 damage or horror
/// from an investigator at your location"* offers only the damage mode to a
/// party carrying no horror. Without the filter the controller could pick the
/// dead mode and spend the action and the supply on a sub-effect that skips.
/// The ability-level gate `effect_can_change_state` applies to a `ChooseOne`
/// with *any-branch* semantics, which is why the ability initiates while one of
/// its modes is dead.
///
/// `OptionId` is an index into the **filtered** list, and resume re-derives it
/// from the same pure-over-`&GameState` predicate before the branch mutates
/// anything, so replay is unaffected.
///
/// **Each option is offered under its branch's own label, anchored to the card
/// the effect is printed on** (#775 / #555). The label comes from
/// [`ChoiceBranch::label`](crate::dsl::ChoiceBranch::label) — authored from the
/// printed text — and the anchor from [`EvalContext::ability_source`], mapped
/// through the one `AbilitySource → OptionTarget` map. A dispatch site that
/// does not know its
/// ability source leaves the options un-anchored, which renders them in the
/// prompt banner exactly as every `ChooseOne` did before.
///
/// **Filtered-to-empty is a skip, not a reject** — the convention
/// [`ground_investigator_choice`] established for the same reason. An
/// activation cannot reach here with every mode dead (the initiation gate
/// proved one live), but a `ChooseOne` nested under a `SkillTest` can: the test
/// has already resolved, and rejecting would unwind it, chaos draw included. A
/// `ChooseOne` with **no branches at all** still rejects — that is a malformed
/// effect, not a board state.
fn step_choose_one(
    cx: &mut Cx,
    branches: &[crate::dsl::ChoiceBranch],
    eval_ctx: EvalContext,
    node: &Effect,
) -> EngineOutcome {
    use crate::engine::dispatch::choice::{
        awaiting_choice_anchored, resolve_choice_count, ChoiceResolution,
    };
    let live: Vec<usize> = branches
        .iter()
        .enumerate()
        .filter(|(_, branch)| effect_can_change_state(cx.state, eval_ctx, &branch.effect))
        .map(|(i, _)| i)
        .collect();
    if live.is_empty() && !branches.is_empty() {
        return EngineOutcome::Done;
    }
    let push_branch = |cx: &mut Cx, i: usize| {
        cx.state
            .continuations
            .push(crate::state::Continuation::Effect(frame_of(
                &branches[i].effect,
                {
                    let mut ctx = eval_ctx;
                    ctx.set_chosen_option(None);
                    ctx
                },
            )));
        EngineOutcome::Done
    };
    match resolve_choice_count(live.len(), cx.state.interactive_acknowledge) {
        ChoiceResolution::Empty => EngineOutcome::Rejected {
            reason: "Effect::ChooseOne with no branches".into(),
        },
        ChoiceResolution::Auto(i) => push_branch(cx, live[i]),
        ChoiceResolution::Suspend => {
            if let Some(crate::engine::OptionId(i)) = eval_ctx.chosen_option() {
                let Some(&branch) = live.get(i as usize) else {
                    return EngineOutcome::Rejected {
                        reason: format!("ChooseOne pick {i} out of range (0..{})", live.len())
                            .into(),
                    };
                };
                push_branch(cx, branch)
            } else {
                let anchor = eval_ctx
                    .ability_source()
                    .map(crate::engine::OptionTarget::from);
                let options = live
                    .iter()
                    .map(|&i| (branches[i].label.clone(), anchor.clone()))
                    .collect();
                suspend_leaf_in_place(cx, node, eval_ctx);
                awaiting_choice_anchored("Choose one", options)
            }
        }
    }
}

/// The [`Effect::Native`] step: dispatch to the card-local handler (threading
/// `ctx.chosen_option` so a resumed native receives its pick). If the native
/// suspends for a choice, **suspend in place** (re-push `node` so resume
/// re-invokes the native with the pick). The native must choose *before* any
/// side effect (standalone contract) so re-invocation is idempotent up to the
/// suspension — no double-apply (#422, #334).
fn step_native(cx: &mut Cx, tag: &str, eval_ctx: EvalContext, node: &Effect) -> EngineOutcome {
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
    let events_before = cx.events.len();
    let outcome = f(cx, &eval_ctx);
    if matches!(outcome, EngineOutcome::AwaitingInput { .. }) {
        // Standalone-contract tripwire (#334): a native that suspends for a pick
        // must do so *before* any side effect, so re-invoking it on resume is
        // idempotent up to the suspension. A native that emitted events then
        // suspended would double-apply them on re-step — flag it loudly.
        debug_assert_eq!(
            cx.events.len(),
            events_before,
            "native {tag:?} pushed events before suspending for a choice; \
             re-invocation on resume would double-apply (standalone-contract violation)",
        );
        suspend_leaf_in_place(cx, node, eval_ctx);
    }
    outcome
}

/// Resolve [`Effect::DrawCards`]: draw `count` cards for the resolved
/// target investigator via the engine's `draw_with_deckout` helper —
/// the same empty-deck path the Draw action and Upkeep step 4.4 use, so
/// a card-effect draw on an empty deck reshuffles the discard, completes
/// the draw, and takes 1 horror on completion (#636). `count == 0`
/// is a clean no-op (no target resolution, no event).
fn draw_cards_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: InvestigatorTarget,
    count: u8,
) -> EngineOutcome {
    if count == 0 {
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
            reason: format!("DrawCards: investigator {target_id:?} is not in the state").into(),
        };
    }
    crate::engine::dispatch::cards::draw_with_deckout(cx, target_id, count);
    EngineOutcome::Done
}

/// Resolve [`Effect::SearchDeck`]: the resolved investigator looks at a deck
/// region (`scope`) ∩ `filter`, takes one eligible card to hand (Rules
/// Reference p.18: obligated if any exist; 0 ⇒ find nothing), then shuffles
/// the deck. The select reuses the Axis-A choice machinery (cursor replay /
/// suspend on 2+), exactly like [`apply_choose_one`]. A `Chosen` target is
/// already bound by [`ground_chosen_targets`]; the take + shuffle are the only
/// mutations and run after the pick resolves.
fn apply_search_deck(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: InvestigatorTarget,
    scope: crate::dsl::SearchScope,
    filter: Option<&crate::dsl::CardFilter>,
    node: &Effect,
) -> EngineOutcome {
    use crate::dsl::SearchScope;
    use crate::engine::dispatch::cards::shuffle_player_deck;
    use crate::engine::dispatch::choice::{
        awaiting_choice, resolve_choice_count, ChoiceResolution,
    };
    use crate::engine::OptionId;

    // 1. Whose deck. `Chosen` is bound by ground_chosen_targets; You/Active
    //    resolve directly.
    let who = match resolve_investigator_target(cx.state, eval_ctx, target) {
        Ok(id) => id,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    let Some(inv) = cx.state.investigators.get(&who) else {
        return EngineOutcome::Rejected {
            reason: format!("SearchDeck: investigator {who:?} is not in the state").into(),
        };
    };

    // 2. Enumerate eligible (deck-index, code) in deck order — deterministic,
    //    so OptionId indices replay across suspend/resume (the deck is not
    //    mutated until step 4).
    let region = match scope {
        SearchScope::Top(n) => usize::from(n).min(inv.deck.len()),
        SearchScope::EntireDeck => inv.deck.len(),
    };
    let eligible: Vec<(usize, crate::state::CardCode)> = inv.deck[..region]
        .iter()
        .enumerate()
        .filter(|(_, code)| match filter {
            None => true,
            Some(f) => filter_matches(f, code),
        })
        .map(|(i, code)| (i, code.clone()))
        .collect();

    // 3. Choice convention — but 0 ⇒ find nothing (not reject).
    let chosen_deck_index: Option<usize> =
        match resolve_choice_count(eligible.len(), cx.state.interactive_acknowledge) {
            ChoiceResolution::Empty => None,
            ChoiceResolution::Auto(i) => Some(eligible[i].0),
            ChoiceResolution::Suspend => {
                if let Some(OptionId(i)) = eval_ctx.chosen_option() {
                    match eligible.get(i as usize) {
                        Some((idx, _)) => Some(*idx),
                        None => {
                            return EngineOutcome::Rejected {
                                reason: format!(
                                    "SearchDeck: pick {i} out of range (0..{})",
                                    eligible.len()
                                )
                                .into(),
                            }
                        }
                    }
                } else {
                    let labels = eligible.iter().map(|(_, c)| c.0.clone()).collect();
                    suspend_leaf_in_place(cx, node, eval_ctx);
                    return awaiting_choice("Search: choose a card to take", labels);
                }
            }
        };

    // 4. Take chosen → hand.
    if let Some(idx) = chosen_deck_index {
        let inv = cx.state.investigators.get_mut(&who).expect("checked above");
        let code = inv.deck.remove(idx);
        inv.hand.push(code.clone());
        cx.events.push(Event::CardSearchedToHand {
            investigator: who,
            code,
        });
    }

    // 5. Shuffle (RR p.18 entire-deck mandatory; Old Book "shuffle the
    //    remaining cards into the deck"). RNG-replayable; no-op on <2 cards.
    shuffle_player_deck(cx, who);
    EngineOutcome::Done
}

/// Whether a deck card `code` matches a [`CardFilter`]: both `trait_` and
/// `kind` (when `Some`) must hold, read from the installed registry's
/// metadata. Returns `false` with no registry (a filtered search finds nothing
/// rather than panicking — only the registry-less test paths, which never use
/// a filter, hit this).
fn filter_matches(f: &crate::dsl::CardFilter, code: &crate::state::CardCode) -> bool {
    let Some(reg) = crate::card_registry::current() else {
        return false;
    };
    let Some(meta) = (reg.metadata_for)(code) else {
        return false;
    };
    if let Some(t) = &f.trait_ {
        if !meta.traits.iter().any(|x| x == t) {
            return false;
        }
    }
    if let Some(k) = f.kind {
        if meta.card_type() != k {
            return false;
        }
    }
    true
}

/// Resolve [`Effect::AttachSelfToLocation`]: the currently-playing event
/// attaches itself to its controller's current location.
///
/// "*This* card" is the one on the **nearest enclosing**
/// [`PlayFromHand`](crate::state::Continuation::PlayFromHand) frame — the play
/// this effect is running inside. Reading the innermost frame rather than a
/// global slot is what makes the answer right when plays nest (#604). The card is
/// **taken** off that frame, so its disposal does not also discard it — one card,
/// no duplicate. Rejects if no play is in progress or the controller is between
/// locations.
///
/// Only `PlayFromHand` frames are considered, and that is exact rather than
/// conservative: an `OnPlay`/`OnEvent` effect always runs above the
/// `PlayFromHand` frame `complete_play` pushed for it, so the nearest such frame
/// is always this effect's own play. The other frames that carry a card mid-play
/// ([`ActionResolution`](crate::state::Continuation::ActionResolution),
/// [`SlotDiscard`](crate::state::Continuation::SlotDiscard)) only ever sit
/// *below* it, and taking from one of those would strand a play that is still
/// going to run.
fn apply_attach_self_to_location(cx: &mut Cx) -> EngineOutcome {
    let Some((frame_idx, investigator)) =
        cx.state
            .continuations
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, frame)| match frame {
                crate::state::Continuation::PlayFromHand {
                    investigator,
                    card: Some(_),
                } => Some((i, *investigator)),
                _ => None,
            })
    else {
        return EngineOutcome::Rejected {
            reason: "AttachSelfToLocation: no card is mid-play".into(),
        };
    };
    let Some(location) = cx
        .state
        .investigators
        .get(&investigator)
        .and_then(|i| i.current_location)
    else {
        return EngineOutcome::Rejected {
            reason: "AttachSelfToLocation: controller has no current location".into(),
        };
    };
    // Validated: take the card off its frame so it is re-homed, not discarded.
    let (code, _owner) = cx.state.continuations[frame_idx]
        .take_play_in_progress(investigator)
        .expect("AttachSelfToLocation: the located frame still holds its card");
    crate::engine::dispatch::threat_area::attach_to_location(cx, location, code);
    EngineOutcome::Done
}

/// Add `amount` to the in-flight skill test's `bonus_attack_damage`
/// accumulator (Vicious Blow 01025). A no-op when there is no in-flight
/// test. The Fight follow-up is the only reader, so this is inert for
/// non-attack tests.
fn boost_attack_damage_effect(cx: &mut Cx, amount: u8) -> EngineOutcome {
    if let Some(test) = cx.state.current_skill_test_mut() {
        test.bonus_attack_damage = test.bonus_attack_damage.saturating_add(amount);
    }
    EngineOutcome::Done
}

/// Add `amount` to the in-flight skill test's `bonus_clues_discovered`
/// accumulator (Deduction 01039) — the clue-side twin of
/// [`boost_attack_damage_effect`]. A no-op when there is no in-flight test.
/// The Investigate follow-up is the only reader, and it *raises its single
/// discovery's count* rather than making a second discovery, so this is inert
/// for non-Investigate tests.
fn discover_additional_clues_effect(cx: &mut Cx, amount: u8) -> EngineOutcome {
    if let Some(test) = cx.state.current_skill_test_mut() {
        test.bonus_clues_discovered = test.bonus_clues_discovered.saturating_add(amount);
    }
    EngineOutcome::Done
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
        // A player-card-type attachment (Barricade 01038 — `Event`) goes to its
        // owner's player discard; an encounter attachment (Obscuring Fog 01168 —
        // `Treachery`) to the encounter discard. Without a registry the type is
        // unknown, so default to the encounter discard (preserves the
        // pre-Barricade behavior).
        let is_player_card = crate::card_registry::current()
            .and_then(|reg| (reg.metadata_for)(&card.code))
            .is_some_and(|m| {
                matches!(
                    m.card_type(),
                    crate::card_data::CardType::Asset
                        | crate::card_data::CardType::Event
                        | crate::card_data::CardType::Skill
                )
            });
        if is_player_card {
            // Solo: the firing controller is the owner. TODO(#371): track the
            // attachment's owner for multiplayer (owner may differ from the
            // leaving investigator).
            if let Some(inv) = cx.state.investigators.get_mut(&eval_ctx.controller) {
                inv.discard.push(card.code.clone());
            }
        } else {
            cx.state.encounter_discard.push(card.code.clone());
        }
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
/// A `Rejected` returned by the branch passes through; any events
/// the branch already pushed (and any state it mutated) are rolled
/// back by `apply_via`'s snapshot-restore at the apply boundary.
/// Which side of the control split the card printed with `code` is on —
/// [`Condition::ControlStatus`]'s reader, and the one the grant sweep asks on
/// behalf of a recipient nobody controls.
///
/// Reads `cards_in_play` only. `glossary/Ownership_and_Control.md` splits the
/// two out-of-play cases — *"A player controls the cards located in his or her
/// out-of-play game areas"* against *"The scenario controls the cards in its
/// out-of-play game areas"* — and `glossary/In_Play_and_Out_of_Play.md` counts
/// *"each encounter card in a investigator's threat area **or at a location**"*
/// as in play. So an encounter card in a threat area sits in an investigator's
/// area while being the scenario's card, and a card put into play *at* a
/// location is under nobody's control at all (#825). The Parlor's *"While Lita
/// Chantler is not controlled by a player"* is therefore true for as long as
/// Lita sits in `Location::cards_at_location`, and false the instant her Parley
/// moves her into a player's `cards_in_play`.
#[must_use]
pub(crate) fn card_control_status(state: &GameState, code: &str) -> ControlStatus {
    let controlled = state
        .investigators
        .values()
        .flat_map(|inv| inv.cards_in_play.iter())
        .any(|card| card.code.as_str() == code);
    if controlled {
        ControlStatus::ByAPlayer
    } else {
        ControlStatus::ByNoPlayer
    }
}

/// Resolve a [`Condition`] against the current state.
///
/// Returns `Err` for conditions that aren't expressible yet (the
/// state shape they'd query against doesn't exist) — the caller
/// turns those into [`EngineOutcome::Rejected`].
pub(crate) fn eval_condition(
    state: &GameState,
    eval_ctx: &EvalContext,
    condition: &Condition,
) -> Result<bool, String> {
    match condition {
        // Board-global: it reads no "you", which is what lets the grant sweep
        // ask it on behalf of a recipient nobody controls (ADR 0014).
        Condition::ControlStatus { code, status } => {
            Ok(card_control_status(state, code) == *status)
        }
        Condition::SkillTestKind(kind) => {
            let t = state.current_skill_test().ok_or_else(|| {
                "Condition::SkillTestKind but no skill test is in flight".to_owned()
            })?;
            Ok(t.kind == *kind)
        }
        Condition::Compare {
            quantity,
            op,
            value,
        } => {
            let lhs = eval_quantity(state, eval_ctx, *quantity);
            let rhs = *value;
            Ok(match op {
                CmpOp::Eq => lhs == rhs,
                CmpOp::Ne => lhs != rhs,
                CmpOp::Lt => lhs < rhs,
                CmpOp::Le => lhs <= rhs,
                CmpOp::Gt => lhs > rhs,
                CmpOp::Ge => lhs >= rhs,
            })
        }
        Condition::Native { tag } => {
            let reg = crate::card_registry::current()
                .ok_or_else(|| format!("Native condition {tag:?}: no card registry installed"))?;
            let predicate = (reg.native_condition_for)(tag)
                .ok_or_else(|| format!("Native condition {tag:?}: no predicate registered"))?;
            Ok(predicate(state, eval_ctx))
        }
        Condition::SkillTest { outcome } => {
            // Inside an [`Trigger::OnSkillTestResolution`] effect, the
            // outcome is already gated by the trigger; using this
            // condition there is redundant. It is *not* unknowable
            // elsewhere in ST.7 — post-#423 the determination lives on
            // [`InFlightSkillTest::resolved`], so an `OnCommit` effect
            // could read it — but nothing wires this condition to that
            // field, and outside an in-flight test (an OnEvent reaction
            // keying off `SkillTestSucceeded`) there is no such frame to
            // read. Reject with a TODO pointing at the preferred trigger.
            Err(format!(
                "TODO: Condition::SkillTest {{ outcome: {outcome:?} }} not yet evaluated; \
                 prefer Trigger::OnSkillTestResolution for resolution-time effects, \
                 or wait for an OnEvent-based reaction model to surface past-test outcome."
            ))
        }
    }
}

/// Resolve a [`Quantity`] against current state for the controller.
/// Always non-negative; returned as `i8` to compose in [`IntExpr`].
/// Used by [`IntExpr::Count`] and [`Condition::Compare`].
fn eval_quantity(state: &GameState, eval_ctx: &EvalContext, q: Quantity) -> i8 {
    let controller = eval_ctx.controller;
    let n: usize = match q {
        // Only the location's own `clues` count. Clues sitting on a *card* at
        // the location — Cover Up 01007's three, in the owner's threat area —
        // are not clues "at" that location, so they must not be swept in here.
        // `data/official-faq/Frequently_Asked_Questions.md`, asked about
        // exactly this pair (Cover Up under Roland's elder sign): *"No.
        // Generally speaking, cards (such as investigators, assets under your
        // control, enemies in your threat area, etc) are 'at' a location.
        // Clues are only 'at' a location if they are physically on that
        // location."* Guarded end-to-end by
        // `clues_on_a_threat_area_card_are_not_clues_at_the_location` in
        // `crates/cards/tests/roland_elder_sign.rs`.
        Quantity::CluesAtControllerLocation => state
            .investigators
            .get(&controller)
            .and_then(|inv| inv.current_location)
            .and_then(|loc| state.locations.get(&loc))
            .map_or(0, |l| usize::from(l.clues)),
        Quantity::EngagedEnemies => state.enemies_engaged_with(controller).count(),
        Quantity::SkillTestFailedBy => usize::from(eval_ctx.failed_by().unwrap_or(0)),
    };
    i8::try_from(n).unwrap_or(i8::MAX)
}

/// Resolve an [`IntExpr`] against the current state for `controller`.
///
/// [`IntExpr::Cond`] evaluates its [`Condition`] (reusing
/// [`eval_condition`]); an unexpressible condition propagates as `Err`,
/// which the caller turns into [`EngineOutcome::Rejected`].
pub(super) fn eval_int_expr(
    state: &GameState,
    eval_ctx: &EvalContext,
    expr: &IntExpr,
) -> Result<i8, String> {
    match expr {
        IntExpr::Lit(n) => Ok(*n),
        IntExpr::Cond {
            when,
            then,
            otherwise,
        } => Ok(if eval_condition(state, eval_ctx, when)? {
            *then
        } else {
            *otherwise
        }),
        IntExpr::Count(q) => Ok(eval_quantity(state, eval_ctx, *q)),
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
/// - [`ModifierScope::ThisSkillTest`]: recorded onto
///   [`GameState::recorded_modifiers`] as a
///   [`RecordedModifier`](crate::state::RecordedModifier) stamped with the
///   in-flight test's [`SkillTestId`](crate::state::SkillTestId), and
///   expired by that test's teardown. This arm is where the card author's
///   [`ModifierScope`] becomes the engine's
///   [`Lifetime`](crate::state::Lifetime) — and therefore where a scope with
///   nothing to stamp is refused (see below).
/// - [`ModifierScope::ThisTurn`]: not yet wired; rejects with TODO
///   until a card or test demands it.
///
/// # A test-scoped modifier with no test
///
/// A buff bought "for this skill test" outside any test has no test to
/// attach to, so it is **rejected** rather than banked. Hyperawareness
/// 01034's *"\[fast\] Spend 1 resource: You get +1 \[intellect\] for this
/// skill test."* is usable at any player window and carries no per-round
/// limit (<https://arkhamdb.com/card/01034>: *"You can use \[fast\] fast
/// actions as many times as you want, as long as you can pay the cost; there
/// is no limit."*), so a row with no identity would surface an unexplained
/// bonus on whatever test came next, several actions or rounds later. The
/// activation paths never reach this rejection — the initiation gate
/// (`effect_can_change_state`) proves the effect inert outside a test and
/// keeps the menu from offering it — but a card whose `OnPlay` or forced
/// effect carries the scope still lands here.
fn modify(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    stat: crate::dsl::Stat,
    delta: i8,
    scope: ModifierScope,
    audience: crate::dsl::ModifierAudience,
) -> EngineOutcome {
    // A recorded row carries only the controller today (see
    // `RecordedModifier`), so a wider audience under a non-constant
    // scope has nowhere to be written down. Reject loudly rather than
    // silently narrowing it to the controller — recorded rows that can
    // name an arbitrary target are #676's.
    if audience != crate::dsl::ModifierAudience::Controller {
        return EngineOutcome::Rejected {
            reason: format!(
                "Modify with audience {audience:?} under scope {scope:?}: only \
                 ModifierAudience::Controller can be recorded as a pending row today. Declare \
                 the wider audience under Trigger::Constant, where the modified-value sweep \
                 finds it. Stat = {stat:?}, delta = {delta}."
            )
            .into(),
        };
    }
    match scope {
        ModifierScope::ThisSkillTest => {
            let Some(test) = cx.state.current_skill_test() else {
                return EngineOutcome::Rejected {
                    reason: format!(
                        "Modify with scope ThisSkillTest resolved with no skill test in flight: \
                         the modifier has no test to attach to, so it cannot be recorded. Stat = \
                         {stat:?}, delta = {delta}."
                    )
                    .into(),
                };
            };
            let lifetime = crate::state::Lifetime::SkillTest(test.id);
            cx.state
                .recorded_modifiers
                .push(crate::state::RecordedModifier::new(
                    eval_ctx.controller,
                    stat,
                    // The DSL's `Modify` carries a literal delta, so every row
                    // written today is a `Lit`. The row is expression-valued
                    // regardless (ADR 0005): what is stored is evaluated at
                    // read time, not at push time.
                    crate::dsl::IntExpr::Lit(delta),
                    lifetime,
                    eval_ctx.source,
                ));
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
            reason: "TODO(#572): ThisTurn scope not yet wired; needs a turn-scoped \
                     accumulator that drains on TurnEnded."
                .into(),
        },
    }
}

/// Apply an [`Effect::AutoResolve`]: latch the in-flight test's
/// [`Determination`].
///
/// `data/rules-reference/rules/glossary/Automatic_Failure_Success.md`:
///
/// > Some card or token abilities may cause a skill test to automatically
/// > fail or to automatically succeed.
///
/// The determination is written as a
/// [`RecordedModifier`](crate::state::RecordedModifier) row stamped with the
/// running test's [`SkillTestId`](crate::state::SkillTestId) — the same
/// population, the same identity check and the same teardown sweep the
/// `[auto_fail]` chaos token's row goes through (#685), so a card-latched
/// determination is the one rule and not a second one. Precedence between a
/// simultaneous failure and success is resolved at *read* time, above the
/// fold, by
/// [`test_determination`](crate::engine::modified_value::test_determination):
/// this arm neither suppresses nor overwrites an existing row, so the answer
/// does not depend on the order two cards latched in (ADR 0007).
///
/// # No window list
///
/// The evaluator does not ask *when* the effect is resolving. The moment
/// comes from the declaring card's own trigger, exactly as
/// [`ModifierScope::ThisSkillTest`] does, and the snapshot's latch range is
/// wider than the rules' "before Step 3" skip clause: Possession 03340
/// latches on commit at ST.2, Delusory Evils 52065 reacts at ST.6.
///
/// # A determination with no test
///
/// **Rejected**, for want of a test identity to stamp — the structural
/// guard, not a defensive one. A determination banked with no test would
/// surface as an unexplained automatic result on whatever test came next,
/// which is the same failure mode a test-scoped `Modify` is refused for.
///
/// The row names the **tester**, not the effect's controller: the
/// determination belongs to the test, and a card may latch one on a test
/// another investigator is taking. `source` is the effect's own, so the
/// event and the row agree on attribution.
fn auto_resolve(cx: &mut Cx, eval_ctx: EvalContext, determination: Determination) -> EngineOutcome {
    let Some(test) = cx.state.current_skill_test() else {
        return EngineOutcome::Rejected {
            reason: format!(
                "AutoResolve resolved with no skill test in flight: there is no test for the \
                 {determination:?} to attach to, so it cannot be recorded."
            )
            .into(),
        };
    };
    let (test_id, investigator) = (test.id, test.investigator);
    cx.state
        .recorded_modifiers
        .push(crate::state::RecordedModifier::determination(
            investigator,
            determination,
            crate::state::Lifetime::SkillTest(test_id),
            eval_ctx.source,
        ));
    cx.events.push(Event::SkillTestDeterminationLatched {
        investigator,
        determination,
        source: eval_ctx.source,
    });
    EngineOutcome::Done
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

    // A discovery is what you actually take, never what you requested: per the
    // Cover Up 01007 / Deduction 01039 FAQ, *"Deduction doesn't allow you to
    // discover clues that aren't at that location. If your location has 1 clue
    // at it, you can only discover 1 clue at most when you investigate it."*
    // Cap **here**, before the timing point, so the would-be-discovery count
    // every replacement effect reads is the real one — Cover Up's "discard that
    // many" derives from `DiscoverClues.count` (#471, ex-#368 item 2).
    //
    // Locked at emission: `perform_discovery`'s own `min` stays as the
    // shrinkage backstop, so a mid-window reaction that *removes* clues shrinks
    // the discovery while one that *adds* clues does not grow it. The
    // `clues == 0` early return above precedes this, so `capped >= 1` and Cover
    // Up is never prompted for a 0-clue discard.
    let capped = count.min(location.clues);

    // Emit the clue-discovery triggering condition and stop: the condition is
    // **coordinator-owned** (#703), so the clues move at the coordinator's
    // resolve step, between the `when` and `at` cells — not here. That is what
    // lets Cover Up 01007's *"[reaction] When you would discover 1 or more clues
    // at your location: Discard that many clues from Cover Up instead"* replace
    // a discovery that has not happened yet.
    //
    // In tail position with nothing after it, per ADR 0003: this returns
    // `Done` when nothing was queued *and* when a `when` window was, and the
    // `drive` loop owns both. The pre-#703 code peeked at the top frame here to
    // tell those apart, which is the shape the ADR forbids.
    crate::engine::dispatch::emit::queue_event(
        cx,
        &crate::engine::dispatch::emit::TimingEvent::DiscoverClues {
            investigator: eval_ctx.controller,
            location: location_id,
            count: capped,
        },
    )
}

/// Set the `pending_cancellation` signal for [`Effect::Cancel`] (Axis D #336).
///
/// A resolution frame must be open: `Cancel` only resolves inside a
/// Before-timing reaction window (via `fire_pending_trigger` /
/// `play_fast_event`), which keeps its frame on the continuation stack until
/// close. The check scans for any window frame's *presence* (not just the top,
/// and ignoring whether candidates remain — the fired candidate is already
/// removed by the time its effect runs).
fn cancel_current_impact(cx: &mut Cx) -> EngineOutcome {
    debug_assert!(
        cx.state
            .continuations
            .iter()
            .any(|c| c.pending_candidates().is_some()),
        "Effect::Cancel evaluated with no open resolution window — a card \
         cancelled outside a Before-timing window (TODO(#367) covers nesting; \
         a malformed card otherwise)"
    );
    cx.state.pending_cancellation = true;
    EngineOutcome::Done
}

/// Move `count` clues (capped at availability) from `location_id` to
/// `controller`, emitting `CluePlaced` + `LocationCluesChanged`.
///
/// The clue-discovery condition's **resolve step** — step 2 of
/// `glossary/Nested_Sequences.md`, called by the timing coordinator between the
/// `when` and `at` cells (`emit::resolve_clue_discovery`, #703), never by
/// [`discover_clue`], which only caps the count and emits. Caller guarantees
/// both ids exist and the location has clues.
///
/// `count` arrives already capped at the location's clues as of the
/// would-be-discovery timing point ([`discover_clue`] caps before emitting, so
/// the count a replacement effect reads is the real one). The `min` here is the
/// **shrinkage backstop** for the gap between emission and this call: a `when`
/// ability that removed clues shrinks the discovery, while one that added clues
/// does not grow it — the quantity is fixed at the moment of the would-be
/// discovery (#471).
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

/// Resolve [`Effect::Deal`]: ground the target investigator and apply `amount`
/// of `kind` (damage or horror) via the elimination helpers (which run the
/// matching defeat check). `amount == 0` is a no-op.
fn deal_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    kind: HarmKind,
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
            reason: format!("Deal: investigator {target_id:?} is not in the state").into(),
        };
    }
    // Dealing damage is the two-step procedure of ADR 0009, walked on a
    // `DealDamage` frame: distribution across soakers + self (#44/K5b-2, prompting
    // per contested point), then the `DamageAssigned` and `DamagePlaced`
    // conditions. This call is therefore **tail position** — the effect walk is
    // parked on its own frame beneath and `Finish` resumes it. The `take_damage` /
    // `take_horror` wrappers still place synchronously and announce nothing (#728).
    let (damage, horror) = match kind {
        HarmKind::Damage => (amount, 0),
        HarmKind::Horror => (0, amount),
    };
    crate::engine::dispatch::combat::begin_deal_damage(
        cx,
        target_id,
        damage,
        horror,
        crate::state::DamageSource::Effect,
    )
}

/// Resolve [`Effect::DealDamageToEnemy`]: ground the chosen enemy (already bound
/// by `ground_chosen_targets`) and deal direct damage via the existing
/// `combat::deal_damage_to_enemy`, attributed to the controller so defeat
/// triggers fire. `amount == 0` is a no-op.
fn deal_damage_to_enemy_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    target: EnemyTarget,
    amount: u8,
) -> EngineOutcome {
    if amount == 0 {
        return EngineOutcome::Done;
    }
    let enemy = match resolve_enemy_target(eval_ctx, target) {
        Ok(e) => e,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    crate::engine::dispatch::combat::deal_damage_to_enemy(
        cx,
        enemy,
        amount,
        Some(eval_ctx.controller),
    );
    EngineOutcome::Done
}

/// Resolve [`Effect::Heal`]: ground the chosen investigator and reduce its
/// `damage`/`horror` by `count`, saturating at 0. Emits [`Event::Healed`] only
/// when something was healed. `count == 0` (or nothing to heal) is a no-op.
fn heal_effect(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    kind: HarmKind,
    target: InvestigatorTarget,
    count: u8,
) -> EngineOutcome {
    if count == 0 {
        return EngineOutcome::Done;
    }
    let id = match resolve_investigator_target(cx.state, eval_ctx, target) {
        Ok(i) => i,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    let Some(inv) = cx.state.investigators.get_mut(&id) else {
        return EngineOutcome::Rejected {
            reason: format!("Heal: investigator {id:?} is not in the state").into(),
        };
    };
    let current = match kind {
        HarmKind::Damage => &mut inv.investigator_card.accumulated_damage,
        HarmKind::Horror => &mut inv.investigator_card.accumulated_horror,
    };
    let healed = (*current).min(count);
    *current -= healed;
    if healed > 0 {
        cx.events.push(Event::Healed {
            investigator: id,
            kind,
            amount: healed,
        });
    }
    EngineOutcome::Done
}

/// Resolve [`Effect::AdvanceCurrentAct`]: advance the act deck. A terminal act
/// advances like any other (ADR 0013) — its reverse is what ends the scenario.
fn apply_advance_current_act(cx: &mut Cx) -> EngineOutcome {
    use crate::engine::dispatch::act_agenda::advance_act;
    if cx.state.act_deck.is_empty() {
        return EngineOutcome::Rejected {
            reason: "AdvanceCurrentAct: no act deck is modeled".into(),
        };
    }
    // AdvanceCurrentAct is only reached from a Forced ability (01110's
    // Ghoul-Priest-defeat advance) — a game-forced advance, so it prompts
    // the on-card flip (#558).
    advance_act(cx, crate::state::AdvanceTrigger::Forced);
    EngineOutcome::Done
}

/// Resolve [`Effect::ReachResolution`]: end the scenario at the printed
/// resolution point. The DSL carries the bare printed number (`card-dsl` has no
/// workspace dependencies); the conversion to
/// [`ResolutionId`](crate::scenario::ResolutionId) happens here, which is the
/// whole of what "converted at the evaluator" means (ADR 0013).
///
/// Latching goes through `end_scenario`, so it is first-writer-wins and pushes
/// the `ScenarioEnd` frame at the bottom of the stack — a resolution point
/// reached from a terminal card's reverse cancels nothing already under way
/// (ADR 0004), including the `AdvanceReverse` frame that fired the reverse.
fn apply_reach_resolution(cx: &mut Cx, n: u8) -> EngineOutcome {
    crate::engine::dispatch::act_agenda::end_scenario(
        cx.state,
        crate::scenario::ScenarioEnding::Resolution(crate::scenario::ResolutionId::new(n)),
    );
    EngineOutcome::Done
}

/// Resolve [`Effect::PlaceDoomOnCurrentAgenda`]: evaluate `count`, place that
/// much doom on the current agenda, and run the doom-threshold check **only if
/// the card printed the advance clause**.
///
/// `data/rules-reference/rules/glossary/Doom.md`:
///
/// > Unless a card otherwise specifies that it can advance the agenda, this is
/// > the only time at which the agenda can advance.
///
/// "This" being the Mythos phase's own check-doom-threshold step, which
/// `phases.rs` runs at 1.3. So the branch here is the card-text distinction
/// itself — Ancient Evils 01166's *"This effect can cause the current agenda to
/// advance."* against Silver Twilight Acolyte 01102's bare placement — and it
/// lives at the card-effect boundary rather than inside a shared helper for
/// exactly that reason.
///
/// A `count` that evaluates to zero or negative is a **full no-op** — it
/// returns before the threshold check, so it cannot advance an agenda already
/// sitting at its threshold on the strength of doom it never placed. The
/// negative clamp is the same one [`Effect::Deal`] applies to its own
/// [`IntExpr`].
///
/// Unlike [`Effect::AdvanceCurrentAct`], an unmodeled agenda deck is **not** a
/// rejection: the placement is a no-op there (both helpers guard), which is
/// what fixtures without an agenda need.
fn apply_place_doom_on_current_agenda(
    cx: &mut Cx,
    eval_ctx: &EvalContext,
    count: &IntExpr,
    may_advance: bool,
) -> EngineOutcome {
    let n = match eval_int_expr(cx.state, eval_ctx, count) {
        Ok(v) => u8::try_from(v.max(0)).unwrap_or(u8::MAX),
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    if n == 0 {
        return EngineOutcome::Done;
    }
    crate::engine::dispatch::act_agenda::place_doom_on_agenda(cx, n);
    if may_advance {
        crate::engine::dispatch::act_agenda::check_doom_threshold(cx);
    }
    EngineOutcome::Done
}

/// Ground any `Chosen` target carried by `effect` before its
/// handler runs (Axis A): enumerate candidates, apply the resolve convention
/// (auto 0/1, suspend on 2+, replay from `cursor`), and bind the choice into
/// the returned [`EvalContext`] (`chosen_investigator` / `chosen_location`)
/// that the handler's target resolver reads. A no-op (returns `eval_ctx`
/// unchanged) for effects with no `Chosen` target, or when the
/// choice is already bound (re-entry within the same evaluation).
///
/// **Candidate scope:** the [`Choose`](crate::dsl::Choose) scope is forwarded
/// to a per-variety enumerator. `Anywhere` offers all investigators / locations;
/// `EntityScope::At(Here)` filters to investigators co-located with the
/// controller and `LocationSet::Here` to the controller's own location (empty —
/// hence a reject — when the controller is between locations). The enemy variety
/// and `LocationSet::YourOrConnecting` land with their consuming PRs (#301 /
/// #306).
fn ground_chosen_targets(
    cx: &mut Cx,
    effect: &Effect,
    eval_ctx: EvalContext,
) -> Result<EvalContext, EngineOutcome> {
    let inv_target = match effect {
        Effect::GainResources { target, .. }
        | Effect::Deal { target, .. }
        | Effect::Heal { target, .. }
        | Effect::DrawCards { target, .. }
        | Effect::SearchDeck { target, .. } => Some(target),
        _ => None,
    };
    if let Some(InvestigatorTarget::Chosen(choose)) = inv_target {
        if eval_ctx.chosen_investigator().is_none() {
            return ground_investigator_choice(cx, eval_ctx, choose.scope, effect);
        }
    }

    if let Effect::DiscoverClue {
        from: LocationTarget::Chosen(choose),
        ..
    } = effect
    {
        if eval_ctx.chosen_location().is_none() {
            return ground_location_choice(cx, eval_ctx, choose.scope);
        }
    }

    if let Effect::DealDamageToEnemy {
        target: EnemyTarget::Chosen(choose),
        ..
    } = effect
    {
        if eval_ctx.chosen_enemy().is_none() {
            return ground_enemy_choice(cx, eval_ctx, choose.scope);
        }
    }

    Ok(eval_ctx)
}

/// Resolve a grounded `*::Chosen` pick against its enumerated candidates
/// (#422): bind `candidates[chosen_option]` (clearing the transient pick), or —
/// on 2+ candidates with no pick yet — return the `AwaitingInput` prompt (the
/// `Leaf` step re-pushes itself as the suspension). `bind` applies the chosen
/// id to the context; resume re-enumerates the same deterministic candidate list
/// and indexes it.
// Six closures/params past the resolver essentials (S5 added the `target`
// anchor); a param struct would obscure the four thin `ground_*` call sites.
#[allow(clippy::too_many_arguments)]
fn resolve_grounded_choice<Id: Copy>(
    eval_ctx: EvalContext,
    candidates: &[Id],
    empty_reason: &'static str,
    prompt: &'static str,
    label: impl Fn(&Id) -> String,
    target: impl Fn(&Id) -> Option<crate::engine::OptionTarget>,
    bind: impl Fn(Id) -> EvalContext,
    interactive: bool,
) -> Result<EvalContext, EngineOutcome> {
    use crate::engine::dispatch::choice::{
        awaiting_choice_anchored, resolve_choice_count, ChoiceResolution,
    };
    match resolve_choice_count(candidates.len(), interactive) {
        ChoiceResolution::Empty => Err(EngineOutcome::Rejected {
            reason: empty_reason.into(),
        }),
        ChoiceResolution::Auto(i) => Ok(bind(candidates[i])),
        ChoiceResolution::Suspend => {
            if let Some(crate::engine::OptionId(i)) = eval_ctx.chosen_option() {
                match candidates.get(i as usize) {
                    Some(&id) => Ok(bind(id)),
                    None => Err(EngineOutcome::Rejected {
                        reason: format!(
                            "{prompt}: pick {i} out of range (0..{})",
                            candidates.len()
                        )
                        .into(),
                    }),
                }
            } else {
                let options = candidates
                    .iter()
                    .map(|id| (label(id), target(id)))
                    .collect();
                Err(awaiting_choice_anchored(prompt, options))
            }
        }
    }
}

/// Ground an `InvestigatorTarget::Chosen` against its [`EntityScope`]:
/// candidates are the matching investigators in sorted `BTreeMap` order (so the
/// `OptionId` index re-derives deterministically). Binds `chosen_investigator`,
/// or suspends in place.
///
/// The scoped list is then filtered by [`investigator_target_eligible`] for the
/// consuming `effect` (RR "Target", #639): an investigator with nothing to heal
/// is not offered as a heal target. The predicate is pure over `&GameState` and
/// the pick is taken before the effect mutates anything, so the filtered list
/// re-derives identically on resume.
///
/// **Filtered-to-empty is a skip, not a reject.** RR "Target" makes an
/// unsatisfiable target requirement a bar on *initiation* — which is
/// `check_activate_ability`'s gate, upstream of here. Reaching resolution with
/// every in-scope candidate ineligible means the ability was legitimately
/// initiated and only this sub-effect has nowhere to land: Medical Texts 01035
/// passes its intellect(2) test with nobody at the location damaged, and its
/// `on_success` heal finds no target. Rejecting there would unwind the whole
/// action — chaos draw included — via `apply_via`'s snapshot restore, so the
/// effect is skipped (`Err(EngineOutcome::Done)`, which `step_leaf` returns
/// after having already popped the leaf) instead. An *empty scope* still
/// rejects, exactly as before this filter existed.
fn ground_investigator_choice(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    scope: crate::dsl::EntityScope,
    effect: &Effect,
) -> Result<EvalContext, EngineOutcome> {
    let in_scope = investigator_candidates(cx.state, eval_ctx.controller, scope);
    let candidates: Vec<_> = in_scope
        .iter()
        .copied()
        .filter(|id| investigator_target_eligible(cx.state, effect, *id))
        .collect();
    if candidates.is_empty() && !in_scope.is_empty() {
        return Err(EngineOutcome::Done);
    }
    resolve_grounded_choice(
        eval_ctx,
        &candidates,
        "Chosen investigator: no candidate in scope",
        "Choose an investigator",
        |id| format!("{id:?}"),
        |_id| None, // investigator-choice anchoring is out of S5 scope
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_investigator(id);
            ctx.set_chosen_option(None);
            ctx
        },
        cx.state.interactive_acknowledge,
    )
}

/// Ground a `LocationTarget::Chosen` against its [`LocationSet`]: candidates are
/// the matching locations in sorted `BTreeMap` order. Binds `chosen_location`,
/// or suspends in place.
fn ground_location_choice(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    set: crate::dsl::LocationSet,
) -> Result<EvalContext, EngineOutcome> {
    let candidates = location_candidates(cx.state, eval_ctx.controller, set);
    resolve_grounded_choice(
        eval_ctx,
        &candidates,
        "Chosen location: no candidate in scope",
        "Choose a location",
        |id| format!("{id:?}"),
        |id| Some(crate::engine::OptionTarget::Location(*id)),
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_location(id);
            ctx.set_chosen_option(None);
            ctx
        },
        cx.state.interactive_acknowledge,
    )
}

/// Ground an `EnemyTarget::Chosen` against its [`EntityScope`]: candidates from
/// `combat::enemies_in_scope`. Binds `chosen_enemy`, or suspends in place.
fn ground_enemy_choice(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    scope: crate::dsl::EntityScope,
) -> Result<EvalContext, EngineOutcome> {
    let candidates =
        crate::engine::dispatch::combat::enemies_in_scope(cx.state, eval_ctx.controller, scope);
    resolve_grounded_choice(
        eval_ctx,
        &candidates,
        "Chosen enemy: no candidate in scope",
        "Choose an enemy",
        |id| format!("{id:?}"),
        |id| Some(crate::engine::OptionTarget::Enemy(*id)),
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_enemy(id);
            ctx.set_chosen_option(None);
            ctx
        },
        cx.state.interactive_acknowledge,
    )
}

/// Ground a designated **Fight**'s target against the co-located-enemy list.
///
/// Candidates are `combat::enemies_in_scope` under
/// [`combat::fight_target_scope`](crate::engine::dispatch::combat::fight_target_scope)
/// — every enemy *at the controller's location* (not engaged-only), in
/// ascending [`EnemyId`] order. Per RR you choose an enemy at your location to
/// attack and need not already be engaged, matching the basic Fight action
/// (#451). Delegates to [`resolve_grounded_choice`]:
/// - 0 candidates → `Rejected` ("Fight: no enemy at your location").
/// - 1 candidate → auto-bind (no suspend; preserves single-enemy behaviour).
/// - 2+ candidates → suspend `AwaitingInput { PickSingle }`.
///
/// On resume the evaluator re-enters the same
/// [`Designated`](crate::state::EffectFrame::Designated) step; `chosen_option`
/// is set and the right branch of `resolve_grounded_choice` picks from the
/// same deterministic list.
fn ground_fight_target_choice(
    cx: &mut Cx,
    eval_ctx: EvalContext,
) -> Result<EvalContext, EngineOutcome> {
    let candidates = crate::engine::dispatch::combat::enemies_in_scope(
        cx.state,
        eval_ctx.controller,
        crate::engine::dispatch::combat::fight_target_scope(),
    );
    resolve_grounded_choice(
        eval_ctx,
        &candidates,
        "Fight: no enemy at your location",
        "Choose an enemy to attack",
        |id| format!("{id:?}"),
        |id| Some(crate::engine::OptionTarget::Enemy(*id)),
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_enemy(id);
            ctx.set_chosen_option(None);
            ctx
        },
        cx.state.interactive_acknowledge,
    )
}

/// Investigators matching an [`EntityScope`](crate::dsl::EntityScope), in
/// `BTreeMap` (id) order so the `OptionId` index replays deterministically.
fn investigator_candidates(
    state: &GameState,
    controller: crate::state::InvestigatorId,
    scope: crate::dsl::EntityScope,
) -> Vec<crate::state::InvestigatorId> {
    use crate::dsl::{EntityScope, LocationSet};
    let EntityScope::At(set) = scope;
    match set {
        LocationSet::Anywhere => state.investigators.keys().copied().collect(),
        LocationSet::Here => match state
            .investigators
            .get(&controller)
            .and_then(|i| i.current_location)
        {
            Some(here) => state
                .investigators
                .iter()
                .filter(|(_, inv)| inv.current_location == Some(here))
                .map(|(id, _)| *id)
                .collect(),
            // controller is between locations ⇒ no "your location"
            None => Vec::new(),
        },
    }
}

/// Locations matching a [`LocationSet`](crate::dsl::LocationSet), in `BTreeMap`
/// (id) order.
fn location_candidates(
    state: &GameState,
    controller: crate::state::InvestigatorId,
    set: crate::dsl::LocationSet,
) -> Vec<crate::state::LocationId> {
    use crate::dsl::LocationSet;
    match set {
        LocationSet::Anywhere => state.locations.keys().copied().collect(),
        // the singleton your-location, or empty when between locations
        LocationSet::Here => state
            .investigators
            .get(&controller)
            .and_then(|i| i.current_location)
            .into_iter()
            .collect(),
    }
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
        InvestigatorTarget::Chosen(_) => ctx.chosen_investigator().ok_or(
            "InvestigatorTarget::Chosen resolved before target-grounding bound it \
             (ground_chosen_targets should run first)",
        ),
    }
}

/// Whether `ability` may initiate, per the Rules Reference initiation gate —
/// the generic [`effect_can_change_state`] check, refined by the ability's
/// optional [`Ability::eligibility`] tag.
///
/// RR p.2 ("Ability" → "Forced Abilities"): *"If a forced ability does not have
/// the potential to change the game state, the ability does not initiate."* RR
/// p.3 states the analogous gate for triggered abilities, so this one predicate
/// serves both the reaction/fast-window offer scan and the forced-trigger scan
/// (#786) — a forced ability whose condition is false must neither resolve nor
/// raise the #466 acknowledge prompt.
///
/// The triggered-ability clause is strictly the stronger of the two — *"and its
/// cost (if any) has the potential to be paid in full, taking active cost
/// modifiers into account"* — and that cost half is **not** modelled here;
/// affordability is checked where a cost is paid, not by this predicate.
///
/// Pure over `&GameState`, which is what lets the reaction side's
/// `withdraw_lapsed_candidates` re-ask it once a sibling option has resolved
/// (#568).
///
/// The tag layer refines opaque [`Effect::Native`] effects the generic gate
/// can't introspect (#368: Cover Up 01007's "if there are any clues on Cover
/// Up", act 01109). No tag → eligible. A tag with no resolvable predicate
/// (registry absent / unknown tag) → suppressed, so a half-installed host never
/// surfaces a gated ability it can't evaluate.
pub(crate) fn ability_can_initiate(
    state: &GameState,
    ability: &Ability,
    source: CandidateSource,
    controller: InvestigatorId,
) -> bool {
    let ctx = EvalContext::for_controller_with_optional_source(controller, source.instance());
    if !effect_can_change_state(state, ctx, &ability.effect) {
        return false;
    }
    let Some(tag) = ability.eligibility.as_deref() else {
        return true;
    };
    let Some(reg) = crate::card_registry::current() else {
        return false;
    };
    let Some(pred) = (reg.native_eligibility_for)(tag) else {
        return false;
    };
    pred(state, &ctx)
}

/// Whether `effect`, resolved against the current `state` and binding `ctx`, has
/// the potential to change the game state.
///
/// This is the generic encoding of the Rules Reference initiation rule (RR p.2:
/// "If a forced ability does not have the potential to change the game state, the
/// ability does not initiate"; RR p.3: "A triggered ability can only be initiated
/// if its effect has the potential to change the game state…"). It gates both the
/// forced-trigger scan and the reaction/fast-window scan, so a no-op ability —
/// forced or triggered — never initiates.
///
/// **Conservative by construction:** returns `true` unless it can *prove* the
/// effect is inert. A meaningful ability is therefore never wrongly suppressed;
/// only provable no-ops are. The proven-no-op set starts with `DiscoverClue`
/// (Roland 01001 at a 0-clue location, #495) and grows one arm per recurring
/// pattern. Opaque [`Effect::Native`] effects fall through to `true` — their
/// no-op detection stays with the card's `eligibility` predicate (#368).
pub(crate) fn effect_can_change_state(
    state: &GameState,
    ctx: EvalContext,
    effect: &Effect,
) -> bool {
    match effect {
        // Discovering 0 clues moves nothing. Otherwise the discovery is inert iff
        // its source location resolves to one with no clues — or genuinely doesn't
        // resolve (a between-locations `YourLocation`, an out-of-test
        // `TestedLocation`). A `Chosen` target is *not yet grounded* at initiation
        // time, so its `Err` is "unknown", not "inert" — assume eligible (no
        // corpus card uses `DiscoverClue { Chosen }` yet; this keeps the
        // conservative invariant honest if one lands).
        Effect::DiscoverClue { from, count } => {
            *count > 0
                && match from {
                    LocationTarget::Chosen(_) => true,
                    _ => resolve_location_target(state, ctx, *from)
                        .ok()
                        .and_then(|id| state.locations.get(&id))
                        .is_some_and(|loc| loc.clues > 0),
                }
        }
        // Investigator-targeted effects whose no-op case is a property of the
        // *target* (#639): inert iff no investigator the target could resolve to
        // is an eligible one. The per-effect judgement lives in
        // [`investigator_target_eligible`] — the single place that decides what
        // "eligible" means — so adding an arm there is all a new such effect
        // needs. Today: `Heal` (nobody carries that harm — First Aid 01019
        // activated by an unharmed solo investigator) and `SearchDeck` (an empty
        // deck finds nothing, draws nothing, and shuffles nothing, since
        // `shuffle_player_deck` is a no-op below 2 cards — Old Book of Lore
        // 01031 on a spent deck).
        Effect::Heal { target, .. } | Effect::SearchDeck { target, .. } => {
            any_eligible_investigator_target(state, ctx, effect, *target)
        }
        // A modifier bought "for this skill test" needs a test to attach to
        // (#676): with none in flight there is no identity to stamp onto the
        // recorded row, so `modify` refuses it and nothing changes. Proving it
        // inert *here* is what keeps the turn menu and the fast-window
        // enumerator from offering Hyperawareness 01034 at a window where
        // clicking it would only cost a resource and reject — the same
        // menu/validator agreement #639 established for the activation gate.
        //
        // `AutoResolve` shares the arm for the same reason on the same gate: a
        // determination latched with no test in flight has no identity to stamp
        // either, so `auto_resolve` refuses it and nothing changes.
        Effect::Modify {
            scope: ModifierScope::ThisSkillTest,
            ..
        }
        | Effect::AutoResolve { .. } => state.current_skill_test().is_some(),
        // A sequence changes state iff any step can (an empty `Seq` is inert).
        Effect::Seq(steps) => steps
            .iter()
            .any(|step| effect_can_change_state(state, ctx, step)),
        // A choice changes state iff some branch can (the controller could pick it).
        Effect::ChooseOne(branches) => branches
            .iter()
            .any(|branch| effect_can_change_state(state, ctx, &branch.effect)),
        // Conservative default: anything not provably inert is assumed to change
        // state, so meaningful abilities are never suppressed.
        _ => true,
    }
}

/// Whether *some* investigator `target` could resolve to is an eligible target
/// of `effect` (per [`investigator_target_eligible`]).
///
/// A `Chosen` target is not yet grounded when the initiation gate asks, so this
/// scans the same candidate list [`ground_investigator_choice`] will enumerate
/// — the two must agree, or the gate would admit an ability whose grounding
/// then rejects for want of a candidate. Once grounding *has* bound a pick
/// (re-entry within an evaluation), the bound investigator is the only one that
/// matters, which is what `resolve_investigator_target` returns.
///
/// A target that does not resolve at all is **unknown, not inert** — same
/// reading the `DiscoverClue` arm gives an ungrounded `Chosen` location — so it
/// stays permitted. Reading a resolution failure as "provably a no-op" would
/// invert the conservative posture and silently suppress a meaningful ability.
fn any_eligible_investigator_target(
    state: &GameState,
    ctx: EvalContext,
    effect: &Effect,
    target: InvestigatorTarget,
) -> bool {
    match target {
        InvestigatorTarget::Chosen(choose) if ctx.chosen_investigator().is_none() => {
            investigator_candidates(state, ctx.controller, choose.scope)
                .into_iter()
                .any(|id| investigator_target_eligible(state, effect, id))
        }
        _ => match resolve_investigator_target(state, ctx, target) {
            Ok(id) => investigator_target_eligible(state, effect, id),
            Err(_) => true,
        },
    }
}

/// Whether investigator `id` is an eligible target of `effect`, per RR
/// "Target": *"A card is not an eligible target for an ability if the
/// resolution of that ability's effect could not change the target's state."*
///
/// Conservative in the same direction as [`effect_can_change_state`]: an effect
/// with no arm here keeps every investigator eligible. Used both by the
/// initiation gate (is there *any* eligible target?) and by
/// [`ground_investigator_choice`] (which ones do we offer?), so the answer is
/// the same in both places.
fn investigator_target_eligible(
    state: &GameState,
    effect: &Effect,
    id: crate::state::InvestigatorId,
) -> bool {
    let Some(inv) = state.investigators.get(&id) else {
        return false;
    };
    match effect {
        Effect::Heal { kind, count, .. } => {
            *count > 0
                && match kind {
                    HarmKind::Damage => inv.damage() > 0,
                    HarmKind::Horror => inv.horror() > 0,
                }
        }
        Effect::SearchDeck { .. } => !inv.deck.is_empty(),
        _ => true,
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
        LocationTarget::Chosen(_) => ctx.chosen_location().ok_or(
            "LocationTarget::Chosen resolved before target-grounding bound it \
             (ground_chosen_targets should run first)",
        ),
        LocationTarget::TestedLocation => state
            .current_skill_test()
            .ok_or("LocationTarget::TestedLocation but no skill test is in flight")
            .and_then(|t| {
                t.tested_location.ok_or(
                    "LocationTarget::TestedLocation but the test's location is unset \
                     (investigator was between locations at test start)",
                )
            }),
    }
}

fn resolve_enemy_target(
    ctx: EvalContext,
    target: EnemyTarget,
) -> Result<crate::state::EnemyId, &'static str> {
    match target {
        EnemyTarget::Chosen(_) => ctx.chosen_enemy().ok_or(
            "EnemyTarget::Chosen resolved before target-grounding bound it \
             (ground_chosen_targets should run first)",
        ),
    }
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
///
/// **"Actions performed" and "actions paid for" are the same number here, and
/// the rules say they need not be.** One activation is charged one surcharge
/// because every corpus ability that designates an action performs exactly one
/// of it. The official FAQ: *"When resolving an ability, the investigator is
/// considered to have performed as many actions as specified by the effect.
/// \[…\] Regardless of the cost paid to initiate the ability, you have
/// performed 3 actions (assuming you took each available action). Conversely,
/// an investigator activating the second ability on Sledgehammer has only
/// performed one action, although they spent two actions to do so."* (`data/official-faq/Frequently_Asked_Questions.md`.) Nothing in
/// Core or Dunwich prints such an ability, so the count is not modelled —
/// whoever adds one must give the surcharge a *performed* count to read rather
/// than inferring it from the action cost paid.
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
        boost_attack_damage, choose_one, constant, deal_damage, deal_damage_to_enemy, deal_horror,
        discover_clue, draw_cards, gain_resources, heal, modify, on_play, search_deck, seq,
        Ability, Choose, Effect, EnemyTarget, HarmKind, InvestigatorTarget, LocationSet,
        LocationTarget, ModifierScope, SkillTestKind, Stat,
    };
    use crate::event::Event;
    use crate::state::{
        CardCode, CardInPlay, CardInstanceId, EnemyId, InvestigatorId, LocationId, SkillKind,
    };
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};
    use crate::{assert_event, assert_no_event};

    use super::{
        effect_can_change_state, eval_condition, eval_int_expr, eval_quantity, push_effect,
        step_effect_frame, EngineOutcome, EvalContext,
    };
    use crate::dsl::Condition;
    use crate::engine::Cx;

    fn ctx(id: u32) -> EvalContext {
        EvalContext::for_controller(InvestigatorId(id))
    }

    /// A state with investigator 1 standing on location 10, which holds `clues`.
    fn state_with_clues_at_location(clues: u8) -> crate::state::GameState {
        let mut inv = test_investigator(1);
        inv.current_location = Some(LocationId(10));
        let mut loc = test_location(10, "Study");
        loc.clues = clues;
        GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build()
    }

    #[test]
    fn discover_clue_can_change_state_only_with_clues_present() {
        // #495 / RR p.2: discovering a clue at your location changes state iff
        // there is a clue to discover.
        let discover = discover_clue(LocationTarget::YourLocation, 1);

        let with_clues = state_with_clues_at_location(2);
        assert!(effect_can_change_state(&with_clues, ctx(1), &discover));

        let no_clues = state_with_clues_at_location(0);
        assert!(!effect_can_change_state(&no_clues, ctx(1), &discover));
    }

    #[test]
    fn discover_zero_count_cannot_change_state() {
        let discover = discover_clue(LocationTarget::YourLocation, 0);
        let with_clues = state_with_clues_at_location(2);
        assert!(!effect_can_change_state(&with_clues, ctx(1), &discover));
    }

    #[test]
    fn seq_can_change_state_iff_any_step_can() {
        let no_clues = state_with_clues_at_location(0);
        // Both steps inert (discover at 0-clue location) → Seq inert.
        let inert = seq(vec![
            discover_clue(LocationTarget::YourLocation, 1),
            discover_clue(LocationTarget::YourLocation, 1),
        ]);
        assert!(!effect_can_change_state(&no_clues, ctx(1), &inert));
        // One step meaningful (gain resources, conservatively state-changing).
        let mixed = seq(vec![
            discover_clue(LocationTarget::YourLocation, 1),
            gain_resources(InvestigatorTarget::You, 1),
        ]);
        assert!(effect_can_change_state(&no_clues, ctx(1), &mixed));
    }

    #[test]
    fn choose_one_can_change_state_iff_any_branch_can() {
        let no_clues = state_with_clues_at_location(0);
        let choice = choose_one([
            (
                "Discover 1 clue",
                discover_clue(LocationTarget::YourLocation, 1),
            ),
            (
                "Gain 1 resource",
                gain_resources(InvestigatorTarget::You, 1),
            ),
        ]);
        assert!(effect_can_change_state(&no_clues, ctx(1), &choice));
    }

    #[test]
    fn unknown_effects_are_conservatively_state_changing() {
        // Default arm: anything we can't prove inert is assumed to change state,
        // so a meaningful ability is never wrongly suppressed.
        let no_clues = state_with_clues_at_location(0);
        assert!(effect_can_change_state(
            &no_clues,
            ctx(1),
            &gain_resources(InvestigatorTarget::You, 1)
        ));
    }

    /// Investigator 1 (and, when `others` is non-empty, further investigators)
    /// on location 10, each seeded `(damage, horror, deck_len)`. #639 fixtures.
    fn state_with_harm(seeds: &[(u32, u8, u8, usize)]) -> crate::state::GameState {
        let mut builder = GameStateBuilder::new().with_location(test_location(10, "Study"));
        for &(id, damage, horror, deck_len) in seeds {
            let mut inv = test_investigator(id);
            inv.current_location = Some(LocationId(10));
            inv.investigator_card.accumulated_damage = damage;
            inv.investigator_card.accumulated_horror = horror;
            inv.deck = (0..deck_len)
                .map(|i| CardCode::new(format!("9000{i}")))
                .collect();
            builder = builder.with_investigator(inv);
        }
        builder.build()
    }

    #[test]
    fn heal_can_change_state_only_when_the_target_carries_that_harm() {
        // #639 / RR "Ability": First Aid's heal on an unharmed investigator has
        // no potential to change the game state.
        let heal_damage = heal(HarmKind::Damage, InvestigatorTarget::You, 1);
        assert!(effect_can_change_state(
            &state_with_harm(&[(1, 2, 0, 0)]),
            ctx(1),
            &heal_damage,
        ));
        assert!(!effect_can_change_state(
            &state_with_harm(&[(1, 0, 0, 0)]),
            ctx(1),
            &heal_damage,
        ));
        // The kinds are independent: horror on the card doesn't make a damage
        // heal meaningful.
        assert!(!effect_can_change_state(
            &state_with_harm(&[(1, 0, 3, 0)]),
            ctx(1),
            &heal_damage,
        ));
    }

    #[test]
    fn heal_of_zero_cannot_change_state() {
        assert!(!effect_can_change_state(
            &state_with_harm(&[(1, 2, 2, 0)]),
            ctx(1),
            &heal(HarmKind::Damage, InvestigatorTarget::You, 0),
        ));
    }

    #[test]
    fn a_chosen_heal_target_scans_every_candidate_in_scope() {
        // Ungrounded `Chosen` at initiation time: the gate asks whether *any*
        // co-located investigator is an eligible target (RR "Target").
        let heal_damage = heal(
            HarmKind::Damage,
            InvestigatorTarget::chosen_at_your_location(),
            1,
        );
        assert!(
            effect_can_change_state(
                &state_with_harm(&[(1, 0, 0, 0), (2, 2, 0, 0)]),
                ctx(1),
                &heal_damage
            ),
            "a co-located damaged investigator keeps the heal live",
        );
        assert!(
            !effect_can_change_state(
                &state_with_harm(&[(1, 0, 0, 0), (2, 0, 0, 0)]),
                ctx(1),
                &heal_damage
            ),
            "nobody at the location has damage ⇒ no eligible target ⇒ inert",
        );
    }

    #[test]
    fn search_deck_is_inert_only_against_an_empty_deck() {
        // #639 / Old Book of Lore 01031: an empty deck has nothing to find and
        // nothing to shuffle. A non-empty one is never proven inert (the
        // mandatory shuffle reorders it even on a fruitless search).
        let search = search_deck(
            InvestigatorTarget::chosen_at_your_location(),
            crate::dsl::SearchScope::Top(3),
            None,
        );
        assert!(effect_can_change_state(
            &state_with_harm(&[(1, 0, 0, 4)]),
            ctx(1),
            &search,
        ));
        assert!(!effect_can_change_state(
            &state_with_harm(&[(1, 0, 0, 0)]),
            ctx(1),
            &search,
        ));
    }

    /// Bounded effect driver — the deleted production `drive_effect_to_base`,
    /// now test-only (Slice D #423). Steps the top contiguous run until it
    /// shrinks to `base` (run complete → `Done`) or a leaf suspends for a pick
    /// (`AwaitingInput`), WITHOUT touching fixture frames beneath `base` (an
    /// in-flight `SkillTest` carrying `tested_location`, say). The production
    /// path no longer needs this — the global `drive` loop drives the parked run
    /// and then *does* advance the enclosing frame — but a unit test parks
    /// fixtures it does not want driven, so it drives bounded instead.
    ///
    /// The run includes the timing coordinator, because a leaf can emit a
    /// triggering condition whose *own resolution* the coordinator performs —
    /// `Effect::DiscoverClue` since #703 only caps the count and emits, and the
    /// clues move at the coordinator's resolve step. Stopping at the `EmitEvent`
    /// frame would leave the effect half-resolved in a way `apply` never does.
    fn drive_effect_run_to(cx: &mut Cx, base: usize) -> EngineOutcome {
        use crate::state::Continuation;
        loop {
            if cx.state.continuations.len() <= base {
                return EngineOutcome::Done;
            }
            let outcome = match cx.state.continuations.last() {
                Some(Continuation::Effect(_)) => step_effect_frame(cx),
                Some(Continuation::EmitEvent { .. }) => {
                    crate::engine::dispatch::coordinator::dispatch_emit_event(cx)
                }
                Some(Continuation::TimingPoint { .. }) => {
                    crate::engine::dispatch::coordinator::dispatch_timing_point(cx)
                }
                // `Effect::Deal` parks one of these and returns in tail position
                // (#727): the two steps of dealing the damage are the frame's,
                // not the effect walk's. The real `drive` loop dispatches it, so
                // this bounded stand-in must too, or `Deal` in a unit test
                // assigns damage that is never placed.
                Some(Continuation::DealDamage { .. }) => {
                    crate::engine::dispatch::combat::drive_deal_damage(cx)
                }
                _ => return EngineOutcome::Done,
            };
            match outcome {
                EngineOutcome::Done => {}
                other => return other,
            }
        }
    }

    /// Push an effect's root frame and drive **only that run** to completion or
    /// a controller-pick suspension — the test-only successor to the deleted
    /// `apply_effect` bounded entry (Slice D #423). `Done` stays `Done`; a 2+
    /// controller pick stays `AwaitingInput`.
    fn run(cx: &mut Cx, effect: &Effect, ctx: EvalContext) -> EngineOutcome {
        let base = cx.state.continuations.len();
        push_effect(cx, effect, ctx);
        drive_effect_run_to(cx, base)
    }

    /// Resume a suspended-in-place effect choice with `PickSingle(i)` — the same
    /// path `apply(ResolveInput)` routes to (#422). Records the pick on the top
    /// `Leaf` via `resume_effect_choice` (which now just cedes to the global
    /// loop), then drives the resumed top effect run **bounded** — in a unit
    /// test there is no `apply()`→`drive()` afterward to step it (Slice D #423).
    fn resume_pick(
        state: &mut crate::state::GameState,
        events: &mut Vec<Event>,
        i: u32,
    ) -> EngineOutcome {
        use crate::state::Continuation;
        let mut cx = Cx { state, events };
        let recorded = crate::engine::dispatch::choice::resume_effect_choice(
            &mut cx,
            &crate::action::InputResponse::PickSingle(crate::engine::OptionId(i)),
        );
        // A reject (bad pick / top not a Leaf) propagates as-is; otherwise the
        // pick is recorded and the resumed run is driven bounded (base = depth
        // just below the top contiguous Effect run, so fixtures stay untouched).
        if !matches!(recorded, EngineOutcome::Done) {
            return recorded;
        }
        let base = cx
            .state
            .continuations
            .iter()
            .rposition(|c| !matches!(c, Continuation::Effect(_)))
            .map_or(0, |idx| idx + 1);
        drive_effect_run_to(&mut cx, base)
    }

    /// Number of options offered by a suspending `AwaitingInput` (replaces the
    /// former `ChoiceFrame.offered.len()` assertion — #422).
    fn offered_count(outcome: &EngineOutcome) -> usize {
        match outcome {
            EngineOutcome::AwaitingInput { request, .. } => request.options.len(),
            other => panic!("expected AwaitingInput, got {other:?}"),
        }
    }

    /// Build a `GameState` with `clue_count` clues at `InvestigatorId(1)`'s location.
    fn with_clues(clue_count: u8) -> crate::state::GameState {
        let loc_id = LocationId(1);
        let mut inv = test_investigator(1);
        inv.current_location = Some(loc_id);
        let mut loc = test_location(1, "Study");
        loc.clues = clue_count;
        GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build()
    }

    /// Assert the top frame is an effect node suspended in place for a pick.
    #[track_caller]
    fn assert_suspended_leaf(state: &crate::state::GameState) {
        assert!(
            matches!(
                state.continuations.last(),
                Some(crate::state::Continuation::Effect(
                    crate::state::EffectFrame::Leaf { .. }
                )),
            ),
            "expected a suspended effect Leaf frame on top, got {:?}",
            state.continuations.last(),
        );
    }

    #[test]
    fn location_has_clues_condition_tracks_clue_count() {
        use card_dsl::dsl::{CmpOp, Quantity};
        let inv_id = InvestigatorId(1);
        let loc_id = LocationId(1);
        let with_clues_local = |clue_count: u8| {
            let mut inv = test_investigator(1);
            inv.current_location = Some(loc_id);
            let mut loc = test_location(1, "Study");
            loc.clues = clue_count;
            GameStateBuilder::new()
                .with_investigator(inv)
                .with_location(loc)
                .build()
        };
        let has_clues = Condition::Compare {
            quantity: Quantity::CluesAtControllerLocation,
            op: CmpOp::Gt,
            value: 0,
        };
        // Condition tracks clue presence at the controller's location.
        assert_eq!(
            eval_condition(
                &with_clues_local(1),
                &EvalContext::for_controller(inv_id),
                &has_clues
            ),
            Ok(true)
        );
        assert_eq!(
            eval_condition(
                &with_clues_local(0),
                &EvalContext::for_controller(inv_id),
                &has_clues
            ),
            Ok(false)
        );
    }

    #[test]
    fn eval_quantity_reads_clues_engaged_and_margin() {
        use card_dsl::dsl::Quantity;
        // clues at location
        let (state, inv) = state_with_cards_in_play(&[]);
        let ctx = EvalContext::for_controller(inv);
        // helper `with_clues(n)` already exists in this module; reuse it:
        assert_eq!(
            eval_quantity(&with_clues(2), &ctx, Quantity::CluesAtControllerLocation),
            2
        );
        assert_eq!(
            eval_quantity(&with_clues(0), &ctx, Quantity::CluesAtControllerLocation),
            0
        );
        // failure margin from the ctx binding
        let mut ctx2 = EvalContext::for_controller(inv);
        ctx2.set_failed_by(3);
        assert_eq!(eval_quantity(&state, &ctx2, Quantity::SkillTestFailedBy), 3);
        assert_eq!(eval_quantity(&state, &ctx, Quantity::SkillTestFailedBy), 0);
    }

    #[test]
    fn eval_count_and_compare_over_clues() {
        use card_dsl::dsl::{CmpOp, Condition, IntExpr, Quantity};
        let (_s, inv) = state_with_cards_in_play(&[]);
        let ctx = EvalContext::for_controller(inv);
        // Count
        assert_eq!(
            eval_int_expr(
                &with_clues(2),
                &ctx,
                &IntExpr::Count(Quantity::CluesAtControllerLocation)
            )
            .unwrap(),
            2
        );
        // Compare: clues > 0
        let has = Condition::Compare {
            quantity: Quantity::CluesAtControllerLocation,
            op: CmpOp::Gt,
            value: 0,
        };
        assert!(eval_condition(&with_clues(1), &ctx, &has).unwrap());
        assert!(!eval_condition(&with_clues(0), &ctx, &has).unwrap());
    }

    #[test]
    fn eval_context_defaults_clue_discovery_count_to_none() {
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        assert_eq!(ctx.clue_discovery_count(), None);
    }

    #[test]
    fn eval_context_round_trips_with_grouped_bindings() {
        let mut ctx = EvalContext::for_controller(InvestigatorId(1));
        ctx.set_failed_by(3);
        ctx.set_chosen_investigator(InvestigatorId(2));
        let json = serde_json::to_string(&ctx).expect("serialize");
        let back: EvalContext = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.failed_by(), Some(3));
        assert_eq!(back.chosen_investigator(), Some(InvestigatorId(2)));
        assert_eq!(back.attacking_enemy(), None);
        assert_eq!(back.chosen_option(), None);
    }

    #[test]
    fn gain_resources_increments_target_wallet_and_emits_event() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let resources_before = state.investigators[&id].resources;
        let mut events = Vec::new();

        let outcome = run(
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

    /// `push_effect` + the real `drive` runs an effect to completion identically
    /// to the (deleted in Slice D) synchronous `apply_effect`: the root frame is
    /// pushed, the global loop steps it, the effect applies, the frame pops.
    #[test]
    fn push_effect_then_drive_runs_to_completion() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let resources_before = state.investigators[&id].resources;
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };

        super::push_effect(&mut cx, &gain_resources(InvestigatorTarget::You, 3), ctx(1));
        assert!(
            matches!(
                cx.state.continuations.last(),
                Some(crate::state::Continuation::Effect(_))
            ),
            "the effect root frame is pushed for the loop",
        );

        let out = crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].resources, resources_before + 3);
        assert!(state.continuations.is_empty(), "effect frame popped");
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

        let outcome = run(
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

        let outcome = run(
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
    fn cancel_effect_sets_pending_cancellation() {
        use crate::state::{Continuation, FastActorScope, FastWindowKind, PhaseStep};
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        // Effect::Cancel asserts an open window frame is present; push a minimal one.
        state.continuations.push(Continuation::FastWindow {
            candidates: Vec::new(),
            fast_actors: FastActorScope::Any,
            kind: FastWindowKind::Phase(PhaseStep::InvestigatorTurnBegins),
        });
        assert!(!state.pending_cancellation);
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &Effect::Cancel,
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.pending_cancellation);
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

        let outcome = run(
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

        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::YourLocation, 1),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        assert!(
            state.open_windows().is_empty(),
            "no before-discover window opens without a registry"
        );
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

        let outcome = run(
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

        let outcome = run(
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

        let outcome = run(
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
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::state::InFlightSkillTest {
                    id: crate::state::SkillTestId(0),
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Intellect,
                    kind: SkillTestKind::Investigate,
                    difficulty_basis: crate::state::DifficultyBasis::Fixed(2),
                    committed_by_active: Vec::new(),
                    tested_location: Some(tested),
                    follow_up: crate::state::SkillTestFollowUp::Investigate,
                    on_fail: None,
                    on_success: None,
                    source: None,
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    bonus_attack_damage: 0,
                    bonus_clues_discovered: 0,
                    resolved: None,
                    symbol_on_fail: None,
                },
            ));
        let mut events = Vec::new();

        let outcome = run(
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

    /// `Effect::BoostAttackDamage` accumulates onto the in-flight test's
    /// `bonus_attack_damage`; repeated applications stack. A no-op with no
    /// in-flight test.
    #[test]
    fn boost_attack_damage_accumulates_on_in_flight_test() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();

        // No in-flight test: a clean no-op (no panic, nothing to mutate).
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &boost_attack_damage(1),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);

        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::state::InFlightSkillTest {
                    id: crate::state::SkillTestId(0),
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Combat,
                    kind: SkillTestKind::Fight,
                    difficulty_basis: crate::state::DifficultyBasis::Fixed(3),
                    committed_by_active: Vec::new(),
                    tested_location: None,
                    follow_up: crate::state::SkillTestFollowUp::None,
                    on_fail: None,
                    on_success: None,
                    source: None,
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    bonus_attack_damage: 0,
                    bonus_clues_discovered: 0,
                    resolved: None,
                    symbol_on_fail: None,
                },
            ));

        for _ in 0..2 {
            run(
                &mut Cx {
                    state: &mut state,
                    events: &mut events,
                },
                &boost_attack_damage(1),
                ctx(1),
            );
        }
        assert_eq!(
            state.current_skill_test().unwrap().bonus_attack_damage,
            2,
            "two BoostAttackDamage(1) applications should stack to 2"
        );
    }

    /// `Effect::DiscoverAdditionalClues` accumulates onto the in-flight test's
    /// `bonus_clues_discovered`; repeated applications stack (two copies of
    /// Deduction 01039 committed to one investigation). A no-op with no
    /// in-flight test.
    #[test]
    fn discover_additional_clues_accumulates_on_in_flight_test() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();

        // No in-flight test: a clean no-op (no panic, nothing to mutate).
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &crate::dsl::discover_additional_clues(1),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);

        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::state::InFlightSkillTest {
                    id: crate::state::SkillTestId(0),
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Intellect,
                    kind: SkillTestKind::Investigate,
                    difficulty_basis: crate::state::DifficultyBasis::Fixed(3),
                    committed_by_active: Vec::new(),
                    tested_location: None,
                    follow_up: crate::state::SkillTestFollowUp::Investigate,
                    on_fail: None,
                    on_success: None,
                    source: None,
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    bonus_attack_damage: 0,
                    bonus_clues_discovered: 0,
                    resolved: None,
                    symbol_on_fail: None,
                },
            ));

        for _ in 0..2 {
            run(
                &mut Cx {
                    state: &mut state,
                    events: &mut events,
                },
                &crate::dsl::discover_additional_clues(1),
                ctx(1),
            );
        }
        assert_eq!(
            state.current_skill_test().unwrap().bonus_clues_discovered,
            2,
            "two DiscoverAdditionalClues(1) applications should stack to 2"
        );
    }

    /// `Effect::DrawCards` moves `count` cards deck→hand for the resolved
    /// target and emits `CardsDrawn`; `count == 0` is a no-op.
    #[test]
    fn draw_cards_effect_draws_for_target() {
        let mut inv = test_investigator(1);
        inv.deck = vec![
            CardCode::new("d1"),
            CardCode::new("d2"),
            CardCode::new("d3"),
        ];
        inv.hand = Vec::new();
        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        let mut events = Vec::new();

        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &draw_cards(InvestigatorTarget::You, 2),
            ctx(1),
        );

        assert_eq!(outcome, EngineOutcome::Done);
        let inv_after = &state.investigators[&InvestigatorId(1)];
        assert_eq!(inv_after.hand.len(), 2, "two cards moved into hand");
        assert_eq!(inv_after.deck.len(), 1, "two cards left the deck");
        assert_event!(events, Event::CardsDrawn { count: 2, .. });

        // count == 0 → clean no-op (no further draw, no event).
        let mut events0 = Vec::new();
        run(
            &mut Cx {
                state: &mut state,
                events: &mut events0,
            },
            &draw_cards(InvestigatorTarget::You, 0),
            ctx(1),
        );
        assert_eq!(state.investigators[&InvestigatorId(1)].hand.len(), 2);
        assert!(events0.is_empty());
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

        let outcome = run(
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
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::state::InFlightSkillTest {
                    id: crate::state::SkillTestId(0),
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Intellect,
                    kind,
                    difficulty_basis: crate::state::DifficultyBasis::Fixed(2),
                    committed_by_active: Vec::new(),
                    tested_location: Some(LocationId(10)),
                    follow_up: crate::state::SkillTestFollowUp::None,
                    on_fail: None,
                    on_success: None,
                    source: None,
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    bonus_attack_damage: 0,
                    bonus_clues_discovered: 0,
                    resolved: None,
                    symbol_on_fail: None,
                },
            ));
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

        let outcome = run(
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

        let outcome = run(
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

        let outcome = run(
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

        let outcome = run(
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

        let outcome = run(
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
        // a bare plain skill test invoked while between locations).
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::state::InFlightSkillTest {
                    id: crate::state::SkillTestId(0),
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Willpower,
                    kind: SkillTestKind::Plain,
                    difficulty_basis: crate::state::DifficultyBasis::Fixed(2),
                    committed_by_active: Vec::new(),
                    tested_location: None,
                    follow_up: crate::state::SkillTestFollowUp::None,
                    on_fail: None,
                    on_success: None,
                    source: None,
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    bonus_attack_damage: 0,
                    bonus_clues_discovered: 0,
                    resolved: None,
                    symbol_on_fail: None,
                },
            ));
        let mut events = Vec::new();

        let outcome = run(
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

        let outcome = run(
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

        let outcome = run(
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
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Willpower, 1, ModifierScope::WhileInPlay),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }

    /// One investigator, with the test identified by `test_id` in flight —
    /// what a `ThisSkillTest` modifier needs in order to have an identity to
    /// be stamped with.
    fn state_during_test(test_id: crate::state::SkillTestId) -> crate::state::GameState {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::test_support::test_skill_test(
                    test_id,
                    InvestigatorId(1),
                    SkillKind::Intellect,
                    SkillTestKind::Plain,
                    2,
                ),
            ));
        state
    }

    #[test]
    fn modify_with_this_skill_test_scope_records_a_row_stamped_with_the_test() {
        let id = InvestigatorId(1);
        let test_id = crate::state::SkillTestId(4);
        let mut state = state_during_test(test_id);
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(events.is_empty(), "recording doesn't emit an event");
        assert_eq!(state.recorded_modifiers.len(), 1);
        let m = &state.recorded_modifiers[0];
        assert_eq!(m.investigator, id);
        assert_eq!(
            m.kind,
            crate::state::RecordedModifierKind::Delta {
                stat: Stat::Intellect,
                delta: crate::dsl::IntExpr::Lit(1),
            },
            "the row stores an expression, not a resolved integer",
        );
        assert_eq!(m.lifetime, crate::state::Lifetime::SkillTest(test_id));
        assert_eq!(m.source, None, "no source on a bare for_controller ctx");
    }

    /// The scope says "for **this** skill test", so with no test in flight
    /// there is no identity to stamp — the modifier is refused rather than
    /// banked onto whatever test comes next (#676).
    #[test]
    fn modify_with_this_skill_test_scope_rejects_outside_a_test() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
            ctx(1),
        );
        assert!(
            matches!(&outcome, EngineOutcome::Rejected { reason }
                if reason.contains("no skill test in flight")),
            "expected a rejection naming the missing test, got {outcome:?}",
        );
        assert!(state.recorded_modifiers.is_empty(), "nothing recorded");
        assert!(events.is_empty());
    }

    #[test]
    fn modify_records_source_when_ctx_has_one() {
        let id = InvestigatorId(1);
        let src = CardInstanceId(42);
        let mut state = state_during_test(crate::state::SkillTestId(0));
        let mut events = Vec::new();
        let ctx_with_src = EvalContext::for_controller_with_source(id, src);
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &modify(Stat::Combat, 2, ModifierScope::ThisSkillTest),
            ctx_with_src,
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.recorded_modifiers[0].source, Some(src));
    }

    #[test]
    fn modify_with_this_turn_scope_rejects_with_todo() {
        let mut state = GameStateBuilder::new().build();
        let mut events = Vec::new();
        let outcome = run(
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
    fn choose_one_single_branch_auto_resolves() {
        // 1 legal option ⇒ auto-bind, no input round-trip.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &choose_one([(
                "Gain 2 resources",
                gain_resources(InvestigatorTarget::You, 2),
            )]),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].resources, before + 2);
        assert!(state.continuations.is_empty(), "no choice frame for auto");
    }

    #[test]
    fn choose_one_offers_only_its_live_branches() {
        // #664: the dead branch (discover at a 0-clue location) is filtered out,
        // leaving one live branch — which auto-resolves rather than prompting.
        let id = InvestigatorId(1);
        let mut state = state_with_clues_at_location(0);
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &choose_one([
                (
                    "Discover 1 clue",
                    discover_clue(LocationTarget::YourLocation, 1),
                ),
                (
                    "Gain 2 resources",
                    gain_resources(InvestigatorTarget::You, 2),
                ),
            ]),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&id].resources,
            before + 2,
            "the sole live branch resolved",
        );
        assert!(
            state.continuations.is_empty(),
            "no prompt for one live branch"
        );
    }

    #[test]
    fn choose_one_with_every_branch_dead_skips() {
        // #664 / #639: filtered-to-empty is a skip, not a reject — rejecting
        // would unwind whatever already resolved above it (a skill test's draw).
        let mut state = state_with_clues_at_location(0);
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &choose_one([
                (
                    "Discover 1 clue",
                    discover_clue(LocationTarget::YourLocation, 1),
                ),
                (
                    "Discover 1 clue again",
                    discover_clue(LocationTarget::YourLocation, 1),
                ),
            ]),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.continuations.is_empty(), "nothing pushed by a skip");
        assert!(events.is_empty(), "a skipped choice changes nothing");
    }

    #[test]
    fn choose_one_with_no_branches_still_rejects() {
        // A branchless ChooseOne is a malformed effect, not a board state — the
        // #664 skip is for a list the *filter* emptied.
        let mut state = state_with_clues_at_location(0);
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &Effect::ChooseOne(vec![]),
            ctx(1),
        );
        assert!(
            matches!(outcome, EngineOutcome::Rejected { .. }),
            "expected Rejected, got {outcome:?}",
        );
    }

    #[test]
    fn choose_one_two_branches_suspends_with_a_choice_frame() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &choose_one([
                (
                    "Gain 1 resource",
                    gain_resources(InvestigatorTarget::You, 1),
                ),
                (
                    "Gain 3 resources",
                    gain_resources(InvestigatorTarget::You, 3),
                ),
            ]),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        // Suspended before any mutation; the ChooseOne Leaf is the prompt.
        assert_eq!(state.investigators[&id].resources, before);
        assert_eq!(offered_count(&outcome), 2);
        assert_suspended_leaf(&state);
    }

    #[test]
    fn choose_one_resumes_the_pick() {
        // Resuming with pick = branch 1 runs the +3 branch.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &choose_one([
                (
                    "Gain 1 resource",
                    gain_resources(InvestigatorTarget::You, 1),
                ),
                (
                    "Gain 3 resources",
                    gain_resources(InvestigatorTarget::You, 3),
                ),
            ]),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].resources, before + 3);
    }

    #[test]
    fn choice_after_earlier_seq_step_no_longer_rejects() {
        // Seq[ GainResources(+1), ChooseOne[ +1, +3 ] ] — a choice *after* a
        // mutating Seq step. The old single-pass replay model rejected this
        // (#346); the frame model suspends on the choice (the +1 already
        // applied) and resumes without double-applying the first step (#422).
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let effect = Effect::Seq(vec![
            gain_resources(InvestigatorTarget::You, 1),
            choose_one([
                (
                    "Gain 1 resource",
                    gain_resources(InvestigatorTarget::You, 1),
                ),
                (
                    "Gain 3 resources",
                    gain_resources(InvestigatorTarget::You, 3),
                ),
            ]),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );
        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "a choice after an earlier Seq step suspends, not rejects: {outcome:?}",
        );
        assert_eq!(
            state.investigators[&id].resources,
            before + 1,
            "the earlier Seq step applied exactly once before the suspend",
        );
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&id].resources,
            before + 1 + 3,
            "resume runs the chosen branch with no double-apply of the first step",
        );
    }

    #[test]
    fn chosen_investigator_single_candidate_auto_binds() {
        // 1 investigator ⇒ auto-bind, no input.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let before = state.investigators[&id].resources;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::chosen_anywhere(), 2),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].resources, before + 2);
        assert!(state.continuations.is_empty());
    }

    #[test]
    fn chosen_investigator_two_candidates_suspends_then_binds_the_pick() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .build();
        let before1 = state.investigators[&InvestigatorId(1)].resources;
        let before2 = state.investigators[&InvestigatorId(2)].resources;
        let mut events = Vec::new();
        // Two candidates ⇒ suspend.
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::chosen_anywhere(), 5),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(
            state.investigators[&InvestigatorId(1)].resources,
            before1,
            "suspend mutates nothing",
        );

        // Resume with pick = option 1 → the second investigator (BTreeMap
        // sorted order) gains.
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&InvestigatorId(2)].resources,
            before2 + 5
        );
        assert_eq!(state.investigators[&InvestigatorId(1)].resources, before1);
    }

    #[test]
    fn choose_one_then_chosen_target_resumes_both_picks() {
        // Two suspensions in one effect (the First Aid shape): a ChooseOne
        // branch pick, then the chosen branch's `*::Chosen` target pick — the
        // case the old single-pass replay model rejected (#346). The parent
        // ChooseOne pop leaves the branch's grounding to suspend independently.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .build();
        let before1 = state.investigators[&InvestigatorId(1)].resources;
        let before2 = state.investigators[&InvestigatorId(2)].resources;
        let effect = choose_one([
            (
                "Gain 1 resource",
                gain_resources(InvestigatorTarget::chosen_anywhere(), 1),
            ),
            (
                "Gain 9 resources",
                gain_resources(InvestigatorTarget::chosen_anywhere(), 9),
            ),
        ]);
        let mut events = Vec::new();

        // Suspend on the branch choice.
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        // Pick branch 1 (+9) → suspends again on its chosen target.
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "second suspend on the target choice: {outcome:?}",
        );
        // Pick target 1 (investigator 2) → completes.
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&InvestigatorId(2)].resources,
            before2 + 9
        );
        assert_eq!(state.investigators[&InvestigatorId(1)].resources, before1);
    }

    #[test]
    fn attach_self_to_location_rejects_with_no_pending_event() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &Effect::AttachSelfToLocation,
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn search_deck_top_n_auto_takes_single_eligible() {
        // One card in the deck top; no filter ⇒ sole eligible ⇒ auto-take.
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.investigators.get_mut(&id).unwrap().deck = vec![CardCode::new("90001")];
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &crate::dsl::search_deck(
                InvestigatorTarget::You,
                crate::dsl::SearchScope::Top(3),
                None,
            ),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        let inv = &state.investigators[&id];
        assert!(inv.hand.contains(&CardCode::new("90001")));
        assert!(inv.deck.is_empty());
    }

    #[test]
    fn search_deck_with_no_eligible_cards_is_find_nothing_not_reject() {
        // Empty deck: 0 eligible ⇒ find nothing, still Done (RR p.18 — a search
        // may legally find nothing; it is NOT a rejection).
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.investigators.get_mut(&id).unwrap().deck.clear();
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &crate::dsl::search_deck(
                InvestigatorTarget::You,
                crate::dsl::SearchScope::Top(3),
                None,
            ),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert!(state.investigators[&id].hand.is_empty());
    }

    #[test]
    fn search_deck_top_n_suspends_on_two_eligible_then_takes_pick() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.investigators.get_mut(&id).unwrap().deck = vec![
            CardCode::new("90001"),
            CardCode::new("90002"),
            CardCode::new("90003"),
        ];
        let mut events = Vec::new();
        let effect = crate::dsl::search_deck(
            InvestigatorTarget::You,
            crate::dsl::SearchScope::Top(3),
            None,
        );
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        let _ = &effect;

        // Resume picking option 1 (the second eligible, "90002").
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert_eq!(outcome, EngineOutcome::Done);
        let inv = &state.investigators[&id];
        assert!(inv.hand.contains(&CardCode::new("90002")));
        assert!(!inv.deck.contains(&CardCode::new("90002")));
        assert_eq!(inv.deck.len(), 2);
    }

    #[test]
    fn two_choices_resume_one_round_trip_at_a_time() {
        // The real client flow: branch choice suspends, resume picks it and
        // suspends *again* on the target choice (a fresh suspended Leaf), resume
        // completes. Drives `resume_effect_choice` (via `resume_pick`) — the same
        // path `apply(ResolveInput)` routes to.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .build();
        let before2 = state.investigators[&InvestigatorId(2)].resources;
        let effect = choose_one([
            (
                "Gain 1 resource",
                gain_resources(InvestigatorTarget::chosen_anywhere(), 1),
            ),
            (
                "Gain 9 resources",
                gain_resources(InvestigatorTarget::chosen_anywhere(), 9),
            ),
        ]);
        let mut events = Vec::new();

        // First suspend: the branch choice.
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &effect,
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));

        // Resume the branch pick (the +9 branch) → suspends again on the
        // target choice (a new suspended Leaf, no replay payload).
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert!(
            matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "second suspend on the target choice: {outcome:?}",
        );
        assert_suspended_leaf(&state);

        // Resume the target pick (investigator 2) → completes.
        let outcome = resume_pick(&mut state, &mut events, 1);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&InvestigatorId(2)].resources,
            before2 + 9
        );
        assert!(state.continuations.is_empty());
    }

    #[test]
    fn chosen_location_two_candidates_suspends() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(test_location(1, "A"))
            .with_location(test_location(2, "B"))
            .build();
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(LocationTarget::chosen_anywhere(), 1),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(offered_count(&outcome), 2, "two locations offered");
        assert_suspended_leaf(&state);
    }

    #[test]
    fn chosen_location_here_auto_binds_the_controllers_location() {
        // Two locations present, but `Here` filters to the controller's own ⇒
        // singleton ⇒ auto-bind (no Choice frame), unlike `Anywhere` which
        // would offer both and suspend.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(test_location(1, "A"))
            .with_location(test_location(2, "B"))
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &discover_clue(
                LocationTarget::Chosen(Choose {
                    scope: LocationSet::Here,
                }),
                1,
            ),
            ctx(1),
        );
        assert!(
            !matches!(outcome, EngineOutcome::AwaitingInput { .. }),
            "Here is a singleton ⇒ auto-binds, never suspends: {outcome:?}",
        );
        assert!(
            state.continuations.is_empty(),
            "no Choice frame for a singleton scope",
        );
    }

    #[test]
    fn chosen_at_your_location_auto_binds_the_sole_co_located_investigator() {
        // Investigator 1 (controller) and 2 are in play; only 1 is at the
        // controller's location. `At(Here)` must offer only investigator 1 and
        // auto-bind it (1 candidate ⇒ no suspend) — `Anywhere` would see 2 and
        // suspend.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(test_location(1, "A"))
            .with_location(test_location(2, "B"))
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .current_location = Some(LocationId(2));
        let before1 = state.investigators[&InvestigatorId(1)].resources;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::chosen_at_your_location(), 2),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&InvestigatorId(1)].resources,
            before1 + 2
        );
        assert!(
            state.continuations.is_empty(),
            "single co-located candidate auto-binds"
        );
    }

    #[test]
    fn chosen_at_your_location_suspends_when_two_are_co_located() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(test_location(1, "A"))
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .current_location = Some(LocationId(1));
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::chosen_at_your_location(), 1),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(
            offered_count(&outcome),
            2,
            "two co-located investigators offered"
        );
        assert_suspended_leaf(&state);
    }

    #[test]
    fn chosen_at_your_location_rejects_when_controller_between_locations() {
        // test_investigator defaults to current_location = None.
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &gain_resources(InvestigatorTarget::chosen_at_your_location(), 1),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(state.continuations.is_empty());
    }

    #[test]
    fn deal_damage_to_chosen_enemy_at_your_location_auto_binds_and_damages() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(test_location(1, "A"))
            .with_location(test_location(2, "B"))
            .with_enemy({
                let mut e = test_enemy(100, "Ghoul");
                e.max_health = 3;
                e.current_location = Some(LocationId(1));
                e
            })
            .with_enemy({
                let mut e = test_enemy(101, "Faraway");
                e.max_health = 3;
                e.current_location = Some(LocationId(2));
                e
            })
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.enemies[&EnemyId(100)].damage,
            1,
            "co-located enemy damaged"
        );
        assert_eq!(
            state.enemies[&EnemyId(101)].damage,
            0,
            "faraway enemy untouched"
        );
        assert!(
            state.continuations.is_empty(),
            "sole co-located candidate auto-binds"
        );
    }

    #[test]
    fn deal_damage_to_chosen_enemy_suspends_when_two_are_co_located() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(test_location(1, "A"))
            .with_enemy({
                let mut e = test_enemy(100, "G1");
                e.current_location = Some(LocationId(1));
                e
            })
            .with_enemy({
                let mut e = test_enemy(101, "G2");
                e.current_location = Some(LocationId(1));
                e
            })
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(offered_count(&outcome), 2, "two co-located enemies offered");
        assert_suspended_leaf(&state);
    }

    #[test]
    fn deal_damage_to_chosen_enemy_rejects_when_none_co_located() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(test_location(1, "A"))
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
        assert!(state.continuations.is_empty());
    }

    #[test]
    fn heal_reduces_horror_saturating_and_emits_event() {
        crate::test_support::install_test_registry();
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .investigator_card
            .accumulated_horror = 1;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            // heal 2 from a 1-horror investigator → saturates to 0, amount 1.
            &heal(HarmKind::Horror, InvestigatorTarget::You, 2),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 0);
        assert_event!(
            events,
            Event::Healed {
                investigator: InvestigatorId(1),
                kind: HarmKind::Horror,
                amount: 1,
            }
        );
    }

    #[test]
    fn heal_target_chosen_at_your_location_auto_binds() {
        crate::test_support::install_test_registry();
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(test_location(1, "A"))
            .with_location(test_location(2, "B"))
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .current_location = Some(LocationId(2));
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .investigator_card
            .accumulated_damage = 2;
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &heal(
                HarmKind::Damage,
                InvestigatorTarget::chosen_at_your_location(),
                1,
            ),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(
            state.investigators[&InvestigatorId(1)].damage(),
            1,
            "sole co-located target healed"
        );
        assert!(state.continuations.is_empty());
    }

    #[test]
    fn heal_target_chosen_suspends_when_two_are_co_located() {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_investigator(test_investigator(2))
            .with_location(test_location(1, "A"))
            .build();
        state
            .investigators
            .get_mut(&InvestigatorId(1))
            .unwrap()
            .current_location = Some(LocationId(1));
        state
            .investigators
            .get_mut(&InvestigatorId(2))
            .unwrap()
            .current_location = Some(LocationId(1));
        // Both carry damage: RR "Target" (#639) makes an investigator with
        // nothing to heal an ineligible target, so a suspend needs two
        // *eligible* candidates, not merely two co-located ones.
        for id in [InvestigatorId(1), InvestigatorId(2)] {
            state
                .investigators
                .get_mut(&id)
                .unwrap()
                .investigator_card
                .accumulated_damage = 2;
        }
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &heal(
                HarmKind::Damage,
                InvestigatorTarget::chosen_at_your_location(),
                1,
            ),
            ctx(1),
        );
        assert!(matches!(outcome, EngineOutcome::AwaitingInput { .. }));
        assert_eq!(
            offered_count(&outcome),
            2,
            "two co-located heal targets offered"
        );
        assert_suspended_leaf(&state);
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
            // A standalone fake investigator card carrying a constant +2
            // willpower — used to prove the unified `controlled_card_instances()`
            // scan now sums the investigator card (not just `cards_in_play`).
            "inv-willpower-plus-2" => Some(vec![constant(modify(
                Stat::Willpower,
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
            ..CardRegistry::EMPTY
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
            run(&mut cx, &super::Effect::DiscardSelf, c)
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
            run(&mut cx, &super::Effect::DiscardSelf, c)
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
        let outcome = run(
            &mut cx,
            &super::Effect::DiscardSelf,
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert!(matches!(outcome, EngineOutcome::Rejected { .. }));
    }

    #[test]
    fn put_into_threat_area_with_clues_seeds_the_placed_instance() {
        use crate::dsl::put_into_threat_area_with_clues;
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let outcome = run(
            &mut cx,
            &put_into_threat_area_with_clues("01007", 3),
            EvalContext::for_controller(id),
        );
        assert!(matches!(outcome, EngineOutcome::Done));
        let placed = state.investigators[&id]
            .threat_area
            .iter()
            .find(|c| c.code.as_str() == "01007")
            .expect("Cover Up placed in threat area");
        assert_eq!(placed.clues, 3, "Cover Up enters with 3 clues");
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
        let outcome = run(
            &mut cx,
            &deal_damage(InvestigatorTarget::You, 2u8),
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].damage(), 2);
        assert_event!(
            events,
            Event::DamageTaken { investigator, amount: 2 } if *investigator == InvestigatorId(1)
        );
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
        let outcome = run(
            &mut cx,
            &deal_horror(InvestigatorTarget::You, 1u8),
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].horror(), 1);
        assert_event!(
            events,
            Event::HorrorTaken { investigator, amount: 1 } if *investigator == InvestigatorId(1)
        );
    }

    #[test]
    fn deal_damage_at_max_health_defeats_investigator() {
        use crate::state::Status;
        // Apply damage that exactly reaches max_health (8 from TEST_INV) via
        // Effect::Deal and assert the investigator is Killed and
        // InvestigatorEliminated is emitted. Pre-load 5 accumulated_damage so
        // 5 + 3 = 8 = defeated with a 3-damage deal.
        crate::test_support::install_test_registry();
        let id = InvestigatorId(1);
        let mut inv = test_investigator(1);
        inv.investigator_card.accumulated_damage = 5;
        let mut state = GameStateBuilder::new().with_investigator(inv).build();
        let mut events = Vec::new();
        let outcome = run(
            &mut Cx {
                state: &mut state,
                events: &mut events,
            },
            &deal_damage(InvestigatorTarget::You, 3u8),
            EvalContext::for_controller(id),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].status, Status::Killed);
        assert_event!(
            events,
            Event::InvestigatorEliminated { investigator, .. } if *investigator == id
        );
    }

    #[test]
    fn deal_amount_can_be_a_count_of_failure_margin() {
        use crate::dsl::{IntExpr, Quantity};
        // Build a Deal whose amount is the failure margin; fail-by 2 → 2 damage.
        let effect = deal_damage(
            InvestigatorTarget::You,
            IntExpr::Count(Quantity::SkillTestFailedBy),
        );
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_active_investigator(InvestigatorId(1))
            .build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let mut eval_ctx = EvalContext::for_controller(InvestigatorId(1));
        eval_ctx.set_failed_by(2);
        let outcome = run(&mut cx, &effect, eval_ctx);
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&InvestigatorId(1)].damage(), 2);
        // Deal evaluates the IntExpr once and applies the result in a single hit;
        // fail-by 2 → amount 2 → one DamageTaken event with amount 2.
        assert_event!(events, Event::DamageTaken { investigator, amount: 2 } if *investigator == InvestigatorId(1));
    }

    #[test]
    fn advance_current_act_non_terminal_bumps_cursor() {
        use crate::state::{Act, CardCode, InvestigatorId};
        use crate::test_support::GameStateBuilder;
        let mut state = GameStateBuilder::new()
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.act_deck = vec![
            Act {
                code: CardCode("a1".into()),
                clue_threshold: 0,
            },
            Act {
                code: CardCode("a2".into()),
                clue_threshold: 0,
            },
        ];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::AdvanceCurrentAct,
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        // The advance is deferred to an AdvanceReverse frame (#482); drive it
        // (no registry ⇒ the reverse fires nothing ⇒ it drives straight through).
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(state.act_index, 1);
        assert!(state.ending.is_none());
    }

    /// `AdvanceCurrentAct` on a **terminal** act (01110's shape) advances it
    /// like any other — the cursor stays because there is no next card, and the
    /// ending comes from the reverse the advance fires, not from the effect
    /// (ADR 0013).
    #[test]
    fn advance_current_act_on_a_terminal_act_lets_its_reverse_end_the_scenario() {
        use crate::scenario::{ResolutionId, ScenarioEnding};
        use crate::state::{Act, InvestigatorId};
        use crate::test_support::{terminal_code, test_investigator, GameStateBuilder};
        crate::test_support::install_test_registry();
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.act_deck = vec![Act {
            code: terminal_code(1),
            clue_threshold: 0,
        }];
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::AdvanceCurrentAct,
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(state.act_index, 0, "terminal act does not move the cursor");
        assert_eq!(
            state.ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(1)))
        );
    }

    /// `Effect::ReachResolution(n)` latches `Resolution(n)` and does nothing
    /// else: the DSL carries the bare printed number and the newtype conversion
    /// happens here (ADR 0013). No deck is modeled, because the effect does not
    /// care which card ran it.
    #[test]
    fn reach_resolution_latches_the_printed_resolution_point() {
        use crate::scenario::{ResolutionId, ScenarioEnding};
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;
        let mut state = GameStateBuilder::new().build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::ReachResolution(3),
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(
            state.ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(3)))
        );
        assert!(events.is_empty(), "the latch emits nothing itself");
    }

    /// It latches through `end_scenario`, so a resolution point already reached
    /// this scenario stands (ADR 0004's first-writer-wins).
    #[test]
    fn reach_resolution_does_not_overwrite_an_ending_already_latched() {
        use crate::scenario::{ResolutionId, ScenarioEnding};
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;
        let mut state = GameStateBuilder::new().build();
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        run(&mut cx, &Effect::ReachResolution(1), ctx);
        run(&mut cx, &Effect::ReachResolution(2), ctx);
        assert_eq!(
            state.ending,
            Some(ScenarioEnding::Resolution(ResolutionId::new(1)))
        );
    }

    /// A two-agenda fixture with the given doom threshold on the current one.
    fn state_with_agenda(threshold: u8) -> crate::state::GameState {
        use crate::state::{Agenda, CardCode, InvestigatorId};
        use crate::test_support::GameStateBuilder;
        let mut state = GameStateBuilder::new()
            .with_turn_order([InvestigatorId(1)])
            .build();
        state.agenda_deck = vec![
            Agenda {
                code: CardCode("ag1".into()),
                doom_threshold: threshold,
            },
            Agenda {
                code: CardCode("ag2".into()),
                doom_threshold: 9,
            },
        ];
        state
    }

    #[test]
    fn place_doom_on_current_agenda_below_threshold_only_places() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(3);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(1),
                may_advance: true,
            },
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.agenda_doom, 1);
        assert_eq!(state.agenda_index, 0, "1 of 3 doom does not advance");
    }

    #[test]
    fn place_doom_on_current_agenda_advances_at_threshold() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(1);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(1),
                may_advance: true,
            },
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        // The advance is deferred to an AdvanceReverse frame (#482); drive it
        // (no registry ⇒ the reverse fires nothing ⇒ it drives straight through).
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(
            state.agenda_index, 1,
            "Ancient Evils 01166: `This effect can cause the current agenda to advance`"
        );
        assert_eq!(state.agenda_doom, 0, "doom reset on advance");
    }

    /// The `may_advance: false` half — Silver Twilight Acolyte 01102's bare
    /// *"Place 1 doom on the current agenda."*, which
    /// `data/rules-reference/rules/glossary/Doom.md` leaves waiting for Mythos:
    ///
    /// > Unless a card otherwise specifies that it can advance the agenda, this
    /// > is the only time at which the agenda can advance.
    ///
    /// So the doom lands and stays landed, even sitting on the threshold.
    #[test]
    fn place_doom_without_the_advance_clause_does_not_advance_at_threshold() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(1);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(1),
                may_advance: false,
            },
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(
            state.agenda_index, 0,
            "no printed advance clause ⇒ the threshold check waits for Mythos 1.3"
        );
        assert_eq!(state.agenda_doom, 1, "the doom is placed, and stays");
    }

    /// Offer of Power 01178's *"place 2 doom"* is one placement of two, so the
    /// threshold is checked once, after both — the second doom is not placed on
    /// an agenda the first already advanced past.
    #[test]
    fn place_doom_places_the_whole_count_before_checking_the_threshold() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(2);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(2),
                may_advance: true,
            },
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(state.agenda_index, 1, "both doom landed, then it advanced");
        assert_eq!(state.agenda_doom, 0);
    }

    /// A computed count is read at resolution, not at declaration — the reason
    /// the variant carries an [`IntExpr`] rather than a `u8`.
    #[test]
    fn place_doom_evaluates_a_computed_count() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(9);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let mut eval_ctx = EvalContext::for_controller(InvestigatorId(1));
        eval_ctx.set_failed_by(3);
        let out = run(
            &mut cx,
            &Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Count(crate::dsl::Quantity::SkillTestFailedBy),
                may_advance: true,
            },
            eval_ctx,
        );
        assert_eq!(out, EngineOutcome::Done);
        assert_eq!(state.agenda_doom, 3);
    }

    /// The point of the variant over a card-local `Native` tag (#716): it
    /// nests. Saracenic Script 02240's act back places its doom as the last
    /// step of a `Seq`; Offer of Power 01178 places 2 inside a `ChooseOne`
    /// branch; Blood on the Altar 02195 places 1 under an `If`-on-failure.
    #[test]
    fn place_doom_composes_as_a_sub_effect() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(9);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::Seq(vec![
                Effect::PlaceDoomOnCurrentAgenda {
                    count: IntExpr::Lit(1),
                    may_advance: true,
                },
                Effect::PlaceDoomOnCurrentAgenda {
                    count: IntExpr::Lit(2),
                    may_advance: true,
                },
            ]),
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(state.agenda_doom, 3);
    }

    /// The `ChooseOne` and `If` halves of the nesting claim. `ChooseOne` with a
    /// single branch auto-resolves, so this needs no input round-trip.
    #[test]
    fn place_doom_nests_in_choose_one_and_if() {
        use crate::dsl::{CmpOp, Condition, IntExpr, Quantity};
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(9);
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let ctx = EvalContext::for_controller(InvestigatorId(1));
        let place = |n| Effect::PlaceDoomOnCurrentAgenda {
            count: IntExpr::Lit(n),
            may_advance: true,
        };
        assert_eq!(
            run(&mut cx, &choose_one([("Place 1 doom", place(1))]), ctx),
            EngineOutcome::Done
        );
        assert_eq!(
            run(
                &mut cx,
                &Effect::If {
                    // No `Condition::Always`; a tautology stands in.
                    condition: Condition::Compare {
                        quantity: Quantity::CluesAtControllerLocation,
                        op: CmpOp::Ge,
                        value: 0,
                    },
                    then: Box::new(place(1)),
                    else_: None,
                },
                ctx,
            ),
            EngineOutcome::Done
        );
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(state.agenda_doom, 2, "both nestings placed their doom");
    }

    /// A zero count is a full no-op, threshold check included: it must not
    /// advance an agenda already sitting at its threshold on the strength of
    /// doom it never placed.
    #[test]
    fn place_doom_of_zero_does_not_run_the_threshold_check() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        let mut state = state_with_agenda(1);
        state.agenda_doom = 1;
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(0),
                may_advance: true,
            },
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done);
        crate::engine::dispatch::drive(&mut cx, EngineOutcome::Done);
        assert_eq!(
            state.agenda_index, 0,
            "no placement ⇒ no check ⇒ no advance"
        );
        assert_eq!(state.agenda_doom, 1);
    }

    #[test]
    fn place_doom_without_an_agenda_deck_is_a_no_op() {
        use crate::dsl::IntExpr;
        use crate::state::InvestigatorId;
        use crate::test_support::GameStateBuilder;
        let mut state = GameStateBuilder::new()
            .with_turn_order([InvestigatorId(1)])
            .build();
        assert!(state.agenda_deck.is_empty());
        let mut events = Vec::new();
        let mut cx = Cx {
            state: &mut state,
            events: &mut events,
        };
        let out = run(
            &mut cx,
            &Effect::PlaceDoomOnCurrentAgenda {
                count: IntExpr::Lit(1),
                may_advance: true,
            },
            EvalContext::for_controller(InvestigatorId(1)),
        );
        assert_eq!(out, EngineOutcome::Done, "no agenda modeled ⇒ no rejection");
        assert_eq!(state.agenda_doom, 0);
    }

    #[test]
    fn grounded_choice_anchors_enemy_options() {
        use crate::engine::{EngineOutcome, OptionTarget};
        let ctx = super::EvalContext::for_controller(InvestigatorId(1));
        let cands = [EnemyId(4), EnemyId(9)];
        let out = super::resolve_grounded_choice(
            ctx,
            &cands,
            "empty",
            "Choose an enemy",
            |id| format!("{id:?}"),
            |id| Some(OptionTarget::Enemy(*id)),
            |_id| ctx,
            false, // 2 candidates → suspend regardless of the flag
        );
        match out {
            Err(EngineOutcome::AwaitingInput { request, .. }) => {
                assert_eq!(
                    request.options[0].target,
                    Some(OptionTarget::Enemy(EnemyId(4)))
                );
                assert_eq!(
                    request.options[1].target,
                    Some(OptionTarget::Enemy(EnemyId(9)))
                );
            }
            other => panic!("2 candidates suspend for a pick, got {other:?}"),
        }
    }

    #[test]
    fn grounded_choice_investigator_stays_unanchored() {
        use crate::engine::EngineOutcome;
        let ctx = super::EvalContext::for_controller(InvestigatorId(1));
        let cands = [InvestigatorId(1), InvestigatorId(2)];
        let out = super::resolve_grounded_choice(
            ctx,
            &cands,
            "empty",
            "Choose an investigator",
            |id| format!("{id:?}"),
            |_id| None, // out of scope for S5
            |_id| ctx,
            false,
        );
        match out {
            Err(EngineOutcome::AwaitingInput { request, .. }) => {
                assert!(request.options.iter().all(|o| o.target.is_none()));
            }
            other => panic!("2 candidates suspend, got {other:?}"),
        }
    }
}
