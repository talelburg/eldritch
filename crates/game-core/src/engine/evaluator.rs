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
//! resolve cleanly, return [`EngineOutcome::Rejected`] with no state
//! change and no events pushed. The outer apply loop's belt-and-
//! suspenders `events.clear()` on rejection backs this up.

use serde::{Deserialize, Serialize};

use crate::card_registry::CardRegistry;
use crate::dsl::{
    CmpOp, Condition, Effect, EnemyTarget, HarmKind, IntExpr, InvestigatorTarget, LocationTarget,
    ModifierScope, Quantity, SkillTestKind, Stat, Trigger,
};
use crate::event::Event;
use crate::state::{GameState, InvestigatorId, SkillKind};

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

/// Clue count a before-discovery interrupt is replacing (bound only while
/// resolving a `WouldDiscoverClues` ability's effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryBinding {
    /// Clues the interrupt is replacing.
    pub clue_discovery_count: u8,
}

/// Attacking enemy bound while resolving an `EnemyAttackDamagedSelf` reaction.
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
    /// Set by [`activate_ability`](crate::engine) so pushed
    /// [`PendingSkillModifier`](crate::state::PendingSkillModifier)
    /// entries can name their source (for replay clarity and future
    /// limit-once-per-test logic). `None` for evaluations not
    /// originating from a specific in-play instance (events played
    /// from hand, scenario forced effects, …).
    pub source: Option<crate::state::CardInstanceId>,
    /// Skill-test margin binding, bound only while running an `on_fail` effect.
    /// Read via [`Self::failed_by`]. `None` outside that window.
    pub skill_test: Option<SkillTestBinding>,
    /// Before-discovery interrupt binding, bound only while resolving a
    /// `WouldDiscoverClues` ability's effect. Read via
    /// [`Self::clue_discovery_count`]. `None` outside that window.
    pub discovery: Option<DiscoveryBinding>,
    /// Enemy-attack reaction binding, bound only while resolving an
    /// `EnemyAttackDamagedSelf` reaction. Read via [`Self::attacking_enemy`].
    /// `None` outside that window. (C5b #237.)
    pub enemy_attack: Option<EnemyAttackBinding>,
    /// Grounded `*::Chosen` picks, bound during a grounded-choice evaluation
    /// (Axis A #334). Read via [`Self::chosen_investigator`] /
    /// [`Self::chosen_location`] / [`Self::chosen_enemy`] /
    /// [`Self::chosen_option`]. `None` outside a grounded choice.
    pub choice: Option<ChoiceBinding>,
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
            skill_test: None,
            discovery: None,
            enemy_attack: None,
            choice: None,
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
}

impl EvalContext {
    /// Just-resolved skill test's failure margin (bound only while running an
    /// `on_fail` effect). Consumed by `IntExpr::Count(Quantity::SkillTestFailedBy)`.
    #[must_use]
    pub fn failed_by(&self) -> Option<u8> {
        self.skill_test.map(|b| b.failed_by)
    }
    /// Clue count a before-discovery interrupt is replacing (bound only while
    /// resolving a `WouldDiscoverClues` ability's effect).
    #[must_use]
    pub fn clue_discovery_count(&self) -> Option<u8> {
        self.discovery.map(|b| b.clue_discovery_count)
    }
    /// Attacking enemy bound while resolving an `EnemyAttackDamagedSelf` reaction.
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

    /// Bind the skill-test failure margin (see [`Self::failed_by`]).
    pub fn set_failed_by(&mut self, margin: u8) {
        self.skill_test = Some(SkillTestBinding { failed_by: margin });
    }
    /// Bind the before-discovery clue count (see [`Self::clue_discovery_count`]).
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
    }
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
        Effect::Modify { stat, delta, scope } => modify(cx, eval_ctx, *stat, *delta, *scope),
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
            i8::try_from(*difficulty).unwrap_or(i8::MAX),
            crate::state::SkillTestFollowUp::None,
            on_success.as_ref().map(|b| (**b).clone()),
            on_fail.as_ref().map(|b| (**b).clone()),
            eval_ctx.source,
            0, // a Revelation skill test takes its difficulty as printed
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
        Effect::Fight {
            combat_modifier,
            extra_damage,
        } => apply_fight(cx, &eval_ctx, combat_modifier, extra_damage),
        Effect::BoostAttackDamage(amount) => boost_attack_damage_effect(cx, *amount),
        Effect::DrawCards { target, count } => draw_cards_effect(cx, eval_ctx, *target, *count),
        Effect::Investigate { shroud_modifier } => {
            apply_investigate(cx, &eval_ctx, shroud_modifier)
        }
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
fn step_choose_one(
    cx: &mut Cx,
    branches: &[Effect],
    eval_ctx: EvalContext,
    node: &Effect,
) -> EngineOutcome {
    use crate::engine::dispatch::choice::{
        awaiting_choice, resolve_choice_count, ChoiceResolution,
    };
    let push_branch = |cx: &mut Cx, i: usize| {
        cx.state
            .continuations
            .push(crate::state::Continuation::Effect(frame_of(
                &branches[i],
                {
                    let mut ctx = eval_ctx;
                    ctx.set_chosen_option(None);
                    ctx
                },
            )));
        EngineOutcome::Done
    };
    match resolve_choice_count(branches.len()) {
        ChoiceResolution::Empty => EngineOutcome::Rejected {
            reason: "Effect::ChooseOne with no branches".into(),
        },
        ChoiceResolution::Auto(i) => push_branch(cx, i),
        ChoiceResolution::Suspend => {
            if let Some(crate::engine::OptionId(i)) = eval_ctx.chosen_option() {
                let i = i as usize;
                if i >= branches.len() {
                    return EngineOutcome::Rejected {
                        reason: format!("ChooseOne pick {i} out of range (0..{})", branches.len())
                            .into(),
                    };
                }
                push_branch(cx, i)
            } else {
                let labels = branches.iter().map(branch_label).collect();
                suspend_leaf_in_place(cx, node, eval_ctx);
                awaiting_choice("Choose one", labels)
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
/// target investigator via the engine's `draw_cards` helper. `count == 0`
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
    crate::engine::dispatch::cards::draw_cards(cx, target_id, count);
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
    let chosen_deck_index: Option<usize> = match resolve_choice_count(eligible.len()) {
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
/// (held in `pending_played_event`) attaches itself to its controller's
/// current location, and is **consumed** from the pending slot so the apply
/// loop's `flush_pending_played_event` does not also discard it — one card, no
/// duplicate. Rejects if no event is mid-play or the controller is between
/// locations.
fn apply_attach_self_to_location(cx: &mut Cx) -> EngineOutcome {
    let Some((investigator, code)) = cx.state.pending_played_event.clone() else {
        return EngineOutcome::Rejected {
            reason: "AttachSelfToLocation: no event is mid-play".into(),
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
    crate::engine::dispatch::threat_area::attach_to_location(cx, location, code);
    // Consume the pending event so it is re-homed, not discarded.
    cx.state.pending_played_event = None;
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

/// Resolve [`Effect::Fight`]: snapshot the combat modifier, read the target
/// enemy from the evaluation context (grounded by `ground_chosen_targets`
/// before this handler runs), and start a Combat skill test whose Fight
/// follow-up deals `1 + extra_damage`. The activation check has already
/// guaranteed ≥1 co-located enemy; `ground_chosen_targets` auto-selects on 1,
/// suspends for a `PickSingle` on 2+, and `chosen_enemy()` is `None` only on
/// 0 (caught pre-cost) — so a missing target here is a state-shape violation
/// rejected loudly rather than silently no-oped.
///
/// Both `combat_modifier` and `extra_damage` are evaluated from an
/// `&IntExpr` (board-state-dependent AST, same path); `extra_damage` is
/// then clamped to `u8` (negative results treated as 0).
fn apply_fight(
    cx: &mut Cx,
    eval_ctx: &EvalContext,
    combat_modifier: &IntExpr,
    extra_damage: &IntExpr,
) -> EngineOutcome {
    let modifier = match eval_int_expr(cx.state, eval_ctx, combat_modifier) {
        Ok(m) => m,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    let extra_damage_n = match eval_int_expr(cx.state, eval_ctx, extra_damage) {
        Ok(v) => u8::try_from(v.max(0)).unwrap_or(u8::MAX),
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    // The target is bound by `ground_chosen_targets` before this handler
    // runs; `None` here means 0 co-located enemies slipped past the pre-cost
    // gate — reject defensively.
    let Some(enemy_id) = eval_ctx.chosen_enemy() else {
        return EngineOutcome::Rejected {
            reason: "Effect::Fight: no co-located enemy chosen (target check skipped?)".into(),
        };
    };
    // `enemy_id` came from `enemies_in_scope` over this same map, so
    // it is present — a silent 0-difficulty default would mask corruption.
    let fight_difficulty = cx
        .state
        .enemies
        .get(&enemy_id)
        .expect("Fight chosen_enemy returned an id absent from state.enemies")
        .fight;
    crate::engine::dispatch::skill_test::start_skill_test(
        cx,
        eval_ctx.controller,
        SkillKind::Combat,
        SkillTestKind::Fight,
        fight_difficulty,
        crate::state::SkillTestFollowUp::Fight {
            enemy: enemy_id,
            extra_damage: extra_damage_n,
        },
        None,
        None,
        eval_ctx.source,
        modifier,
    )
}

/// Resolve [`Effect::Investigate`]: apply `shroud_modifier` to the
/// controller's location difficulty and start an Investigate skill test
/// (reusing the base Investigate follow-up, so success discovers a clue).
/// The modifier adjusts the *location difficulty* (shroud), not the
/// investigator's total — the reduced shroud clamps at 0 (RR p.4: game
/// values can never be reduced below 0). The activation check has already
/// confirmed a revealed location to test, so the missing/unrevealed cases
/// here are defensive (state-shape) rejections.
fn apply_investigate(
    cx: &mut Cx,
    eval_ctx: &EvalContext,
    shroud_modifier: &IntExpr,
) -> EngineOutcome {
    let delta = match eval_int_expr(cx.state, eval_ctx, shroud_modifier) {
        Ok(d) => d,
        Err(reason) => {
            return EngineOutcome::Rejected {
                reason: reason.into(),
            }
        }
    };
    let Some(location_id) = cx
        .state
        .investigators
        .get(&eval_ctx.controller)
        .and_then(|inv| inv.current_location)
    else {
        return EngineOutcome::Rejected {
            reason: "Effect::Investigate: controller has no current location".into(),
        };
    };
    let Some(location) = cx.state.locations.get(&location_id) else {
        return EngineOutcome::Rejected {
            reason: format!("Effect::Investigate: location {location_id:?} is not in state").into(),
        };
    };
    if !location.revealed {
        return EngineOutcome::Rejected {
            reason: format!("Effect::Investigate: location {location_id:?} is not revealed").into(),
        };
    }
    // Effective shroud folds in attachment modifiers (Obscuring Fog); fall
    // back to the printed value with no registry (bare unit tests), matching
    // the base Investigate action. Shroud is u8 in state but difficulty is
    // i8; saturate the conversion (realistic shrouds are 0–6), apply the
    // (negative) modifier, then clamp the reduced difficulty at 0.
    let shroud = match crate::card_registry::current() {
        Some(reg) => effective_shroud(reg, location),
        None => location.shroud,
    };
    let difficulty = i8::try_from(shroud)
        .unwrap_or(i8::MAX)
        .saturating_add(delta)
        .max(0);
    crate::engine::dispatch::skill_test::start_skill_test(
        cx,
        eval_ctx.controller,
        SkillKind::Intellect,
        SkillTestKind::Investigate,
        difficulty,
        crate::state::SkillTestFollowUp::Investigate,
        None,
        None,
        eval_ctx.source,
        0,
    )
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
/// Inherits [`apply_seq`]'s partial-events-on-rejection caveat: a
/// `Rejected` returned by the branch passes through with whatever
/// events the branch already pushed. The structural fix lives at
/// the outer `apply` loop (TODO in `engine/mod.rs::apply`).
/// Resolve a [`Condition`] against the current state.
///
/// Returns `Err` for conditions that aren't expressible yet (the
/// state shape they'd query against doesn't exist) — the caller
/// turns those into [`EngineOutcome::Rejected`].
fn eval_condition(
    state: &GameState,
    eval_ctx: &EvalContext,
    condition: &Condition,
) -> Result<bool, String> {
    match condition {
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

/// Resolve a [`Quantity`] against current state for the controller.
/// Always non-negative; returned as `i8` to compose in [`IntExpr`].
/// Used by [`IntExpr::Count`] and [`Condition::Compare`].
fn eval_quantity(state: &GameState, eval_ctx: &EvalContext, q: Quantity) -> i8 {
    let controller = eval_ctx.controller;
    let n: usize = match q {
        Quantity::CluesAtControllerLocation => state
            .investigators
            .get(&controller)
            .and_then(|inv| inv.current_location)
            .and_then(|loc| state.locations.get(&loc))
            .map_or(0, |l| usize::from(l.clues)),
        Quantity::EngagedEnemies => state
            .enemies
            .values()
            .filter(|e| e.engaged_with == Some(controller))
            .count(),
        Quantity::SkillTestFailedBy => usize::from(eval_ctx.failed_by().unwrap_or(0)),
    };
    i8::try_from(n).unwrap_or(i8::MAX)
}

/// Resolve an [`IntExpr`] against the current state for `controller`.
///
/// [`IntExpr::Cond`] evaluates its [`Condition`] (reusing
/// [`eval_condition`]); an unexpressible condition propagates as `Err`,
/// which the caller turns into [`EngineOutcome::Rejected`].
fn eval_int_expr(state: &GameState, eval_ctx: &EvalContext, expr: &IntExpr) -> Result<i8, String> {
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

    // Before-timing clue-discovery window (Cover Up 01007; Axis D #336,
    // migrated from the C5a `clue_interrupt` seam). Reaction-only Before timing
    // point: `emit_event` queues the window iff an eligible `WouldDiscoverClues`
    // reaction is controlled at the discovery location — the "at your location"
    // scoping and the `card.clues > 0` potential-gate stand-in (RR p.2;
    // TODO(#368)) live in the window scan. If the window opened, suspend; the
    // `BeforeDiscoverClues` continuation performs the deferred discovery on
    // close (unless a reaction cancelled it). No registry / no eligible card →
    // `open_windows` stays empty and the discovery happens now.
    let _ = crate::engine::dispatch::emit::emit_event(
        cx,
        &crate::engine::dispatch::emit::TimingEvent::WouldDiscoverClues {
            investigator: eval_ctx.controller,
            location: location_id,
            count,
        },
    );
    // `emit_event` pushes the before-discover window (if any eligible reaction
    // matched) on *top* of the stack. Check the top window's kind rather than
    // "any window open" — `discover_clue` can run while an *outer* reaction
    // window is already open (e.g. Evidence! 01022 played in an after-defeat
    // window then discovers a clue), and that outer window must not be mistaken
    // for a queued before-discover window.
    if matches!(
        cx.state
            .continuations
            .last()
            .and_then(crate::state::Continuation::window_timing_event),
        Some(crate::engine::TimingEvent::WouldDiscoverClues { .. })
    ) {
        return crate::engine::dispatch::reaction_windows::open_queued_reaction_window(cx);
    }

    perform_discovery(cx, location_id, count, eval_ctx.controller);
    EngineOutcome::Done
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
    // Interactive distribution across soakers + self (#44 / K5b-2): prompt when a
    // soaker can take a contested point, else place synchronously. The harm path
    // (soak-first + investigator defeat on a lethal share) is unchanged — only
    // the *interactivity* is added vs. the K5a `take_damage`/`take_horror`
    // wrappers (still used by the deferred loop sites).
    let (damage, horror) = match kind {
        HarmKind::Damage => (amount, 0),
        HarmKind::Horror => (0, amount),
    };
    crate::engine::dispatch::combat::soak_and_distribute(
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

/// Resolve [`Effect::AdvanceCurrentAct`]: latch a resolution if the current
/// act carries one, else advance the act deck.
fn apply_advance_current_act(cx: &mut Cx) -> EngineOutcome {
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

/// A render label for one [`Effect::ChooseOne`] branch. The `Debug` form is
/// adequate for v0 host rendering; refine only when a card needs prettier
/// text.
fn branch_label(effect: &Effect) -> String {
    format!("{effect:?}")
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
            return ground_investigator_choice(cx, eval_ctx, choose.scope);
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

    if let Effect::Fight { .. } = effect {
        if eval_ctx.chosen_enemy().is_none() {
            return ground_fight_target_choice(cx, eval_ctx);
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
fn resolve_grounded_choice<Id: Copy>(
    eval_ctx: EvalContext,
    candidates: &[Id],
    empty_reason: &'static str,
    prompt: &'static str,
    label: impl Fn(&Id) -> String,
    bind: impl Fn(Id) -> EvalContext,
) -> Result<EvalContext, EngineOutcome> {
    use crate::engine::dispatch::choice::{
        awaiting_choice, resolve_choice_count, ChoiceResolution,
    };
    match resolve_choice_count(candidates.len()) {
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
                let labels = candidates.iter().map(label).collect();
                Err(awaiting_choice(prompt, labels))
            }
        }
    }
}

/// Ground an `InvestigatorTarget::Chosen` against its [`EntityScope`]:
/// candidates are the matching investigators in sorted `BTreeMap` order (so the
/// `OptionId` index re-derives deterministically). Binds `chosen_investigator`,
/// or suspends in place.
fn ground_investigator_choice(
    cx: &mut Cx,
    eval_ctx: EvalContext,
    scope: crate::dsl::EntityScope,
) -> Result<EvalContext, EngineOutcome> {
    let candidates = investigator_candidates(cx.state, eval_ctx.controller, scope);
    resolve_grounded_choice(
        eval_ctx,
        &candidates,
        "Chosen investigator: no candidate in scope",
        "Choose an investigator",
        |id| format!("{id:?}"),
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_investigator(id);
            ctx.set_chosen_option(None);
            ctx
        },
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
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_location(id);
            ctx.set_chosen_option(None);
            ctx
        },
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
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_enemy(id);
            ctx.set_chosen_option(None);
            ctx
        },
    )
}

/// Ground the [`Effect::Fight`] target against the co-located-enemy list.
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
/// On resume the evaluator re-enters the same `Leaf` step; `chosen_option`
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
        |id| {
            let mut ctx = eval_ctx;
            ctx.set_chosen_enemy(id);
            ctx.set_chosen_option(None);
            ctx
        },
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

/// The controller's **elder-sign** skill-test modifier: the
/// `IntExpr` on their investigator card's `Trigger::ElderSign { modifier }`
/// ability, evaluated for the controller. Returns `0` when the controller is
/// not found, the card isn't in the registry, or it carries no elder-sign
/// ability — so every investigator without an elder-sign resolves as 0.
///
/// Called from the skill-test resolution's `TokenResolution::ElderSign` arm
/// (`skill_test.rs`); the bonus flows through the existing `Modifier` total.
///
/// **Scope (#118), sunset by #448:** handles only pure-modifier elder-signs.
/// Signs that also run an effect (Daisy / Agnes) are deferred — see
/// [`Trigger::ElderSign`](crate::dsl::Trigger::ElderSign).
#[must_use]
pub(crate) fn elder_sign_modifier(
    state: &GameState,
    registry: &CardRegistry,
    controller: InvestigatorId,
) -> i8 {
    let Some(inv) = state.investigators.get(&controller) else {
        return 0;
    };
    let Some(abilities) = (registry.abilities_for)(&inv.investigator_card.code) else {
        return 0;
    };
    let ctx = EvalContext::for_controller(controller);
    for ability in &abilities {
        if let Trigger::ElderSign { modifier } = &ability.trigger {
            // A malformed elder-sign IntExpr (unexpressible Condition) yields
            // Err; treat it as no bonus rather than panicking mid-test — the
            // only in-scope IntExpr is Count(CluesAtControllerLocation), which
            // is always Ok.
            return eval_int_expr(state, &ctx, modifier).unwrap_or(0);
        }
    }
    0
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
/// `Trigger::Constant` `Effect::Modify` on every instance `controller`
/// controls — the investigator card, cards in play, and the threat area
/// (via [`controlled_card_instances`](crate::state::Investigator::controlled_card_instances),
/// #448 cp3a) — whose scope and stat both satisfy the given predicates. Silently skips
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
    for in_play in inv.controlled_card_instances() {
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
        boost_attack_damage, constant, deal_damage, deal_damage_to_enemy, deal_horror,
        discover_clue, draw_cards, gain_resources, heal, modify, on_play, seq, Ability, Choose,
        Effect, EnemyTarget, HarmKind, InvestigatorTarget, LocationSet, LocationTarget,
        ModifierScope, SkillTestKind, Stat,
    };
    use crate::event::Event;
    use crate::state::{
        CardCode, CardInPlay, CardInstanceId, EnemyId, InvestigatorId, LocationId, SkillKind,
    };
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};
    use crate::{assert_event, assert_no_event};

    use super::{
        constant_skill_modifier, effective_shroud, eval_condition, eval_int_expr, eval_quantity,
        push_effect, step_effect_frame, unconditional_constant_stat_modifier, EngineOutcome,
        EvalContext,
    };
    use crate::dsl::Condition;
    use crate::engine::Cx;

    fn ctx(id: u32) -> EvalContext {
        EvalContext::for_controller(InvestigatorId(id))
    }

    /// Bounded effect driver — the deleted production `drive_effect_to_base`,
    /// now test-only (Slice D #423). Steps the top contiguous `Effect` run until
    /// it shrinks to `base` (run complete → `Done`) or a leaf suspends for a pick
    /// (`AwaitingInput`), WITHOUT touching fixture frames beneath `base` (an
    /// in-flight `SkillTest` carrying `tested_location`, say). The production
    /// path no longer needs this — the global `drive` loop drives the parked run
    /// and then *does* advance the enclosing frame — but a unit test parks
    /// fixtures it does not want driven, so it drives bounded instead.
    fn drive_effect_run_to(cx: &mut Cx, base: usize) -> EngineOutcome {
        use crate::state::Continuation;
        loop {
            if cx.state.continuations.len() <= base
                || !matches!(cx.state.continuations.last(), Some(Continuation::Effect(_)))
            {
                return EngineOutcome::Done;
            }
            match step_effect_frame(cx) {
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
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    test_modifier: 0,
                    bonus_attack_damage: 0,
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
                    investigator: InvestigatorId(1),
                    skill: SkillKind::Combat,
                    kind: SkillTestKind::Fight,
                    difficulty: 3,
                    committed_by_active: Vec::new(),
                    tested_location: None,
                    follow_up: crate::state::SkillTestFollowUp::None,
                    on_fail: None,
                    on_success: None,
                    source: None,
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    test_modifier: 0,
                    bonus_attack_damage: 0,
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
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    test_modifier: 0,
                    bonus_attack_damage: 0,
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
        // bare PerformSkillTest invoked while between locations).
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::state::InFlightSkillTest {
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
                    continuation: crate::state::SkillTestStep::AwaitingCommit,
                    test_modifier: 0,
                    bonus_attack_damage: 0,
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

    #[test]
    fn modify_with_this_skill_test_scope_pushes_pending_modifier() {
        let id = InvestigatorId(1);
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
        let outcome = run(
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
            &Effect::ChooseOne(vec![gain_resources(InvestigatorTarget::You, 2)]),
            ctx(1),
        );
        assert_eq!(outcome, EngineOutcome::Done);
        assert_eq!(state.investigators[&id].resources, before + 2);
        assert!(state.continuations.is_empty(), "no choice frame for auto");
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
            &Effect::ChooseOne(vec![
                gain_resources(InvestigatorTarget::You, 1),
                gain_resources(InvestigatorTarget::You, 3),
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
            &Effect::ChooseOne(vec![
                gain_resources(InvestigatorTarget::You, 1),
                gain_resources(InvestigatorTarget::You, 3),
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
            Effect::ChooseOne(vec![
                gain_resources(InvestigatorTarget::You, 1),
                gain_resources(InvestigatorTarget::You, 3),
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
        let effect = Effect::ChooseOne(vec![
            gain_resources(InvestigatorTarget::chosen_anywhere(), 1),
            gain_resources(InvestigatorTarget::chosen_anywhere(), 9),
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
        let effect = Effect::ChooseOne(vec![
            gain_resources(InvestigatorTarget::chosen_anywhere(), 1),
            gain_resources(InvestigatorTarget::chosen_anywhere(), 9),
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
    fn a_seated_investigator_card_constant_modifier_is_summed() {
        // The investigator card lives in `investigator_card`, NOT in
        // `cards_in_play`. After cp3a the constant-modifier scan walks
        // `controlled_card_instances()`, which yields the investigator card
        // first, so its `Trigger::Constant` modifier must be summed without any
        // `cards_in_play` injection.
        let (mut state, id) = state_with_cards_in_play(&[]);
        state
            .investigators
            .get_mut(&id)
            .unwrap()
            .investigator_card
            .code = CardCode::new("inv-willpower-plus-2");
        let reg = fake_registry();
        assert!(
            state.investigators[&id].cards_in_play.is_empty(),
            "the modifier must come from the investigator card, not cards_in_play"
        );
        assert_eq!(
            constant_skill_modifier(&state, &reg, id, SkillKind::Willpower, SkillTestKind::Plain),
            2
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
        // InvestigatorDefeated is emitted. Pre-load 5 accumulated_damage so
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
            Event::InvestigatorDefeated { investigator, .. } if *investigator == id
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
            },
            Act {
                code: CardCode("a2".into()),
                clue_threshold: 0,
                resolution: Some(Resolution::Won { id: "R1".into() }),
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
        assert_eq!(state.act_index, 0, "terminal act does not move the cursor");
        assert!(matches!(state.resolution, Some(Resolution::Won { .. })));
    }

    /// `elder_sign_modifier` reads the controller's investigator card's
    /// `Trigger::ElderSign { modifier }` and evaluates it. Roland's
    /// `Count(CluesAtControllerLocation)` returns the clue count at his
    /// location; an investigator with no elder-sign ability returns 0.
    #[test]
    fn elder_sign_modifier_reads_controller_card_clue_count() {
        use crate::dsl::{elder_sign, IntExpr, Quantity};
        use crate::state::CardCode;

        // Mock registry: code "ES" carries a Count(CluesAtControllerLocation)
        // elder-sign; everything else has no abilities.
        fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
            if code.as_str() == "ES" {
                Some(vec![elder_sign(IntExpr::Count(
                    Quantity::CluesAtControllerLocation,
                ))])
            } else {
                None
            }
        }
        fn metadata_for(_: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
            None
        }
        let registry = CardRegistry {
            metadata_for,
            abilities_for,
            native_effect_for: |_| None,
        };

        let inv_id = InvestigatorId(1);
        let loc_id = crate::state::LocationId(10);
        let mut inv = test_investigator(1);
        inv.investigator_card.code = CardCode::new("ES");
        inv.current_location = Some(loc_id);
        let mut loc = test_location(10, "Study");
        loc.clues = 2;
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build();

        assert_eq!(super::elder_sign_modifier(&state, &registry, inv_id), 2);

        // An investigator whose card has no elder-sign ability → 0.
        let inv_id2 = InvestigatorId(2);
        let mut inv2 = test_investigator(2);
        inv2.investigator_card.code = CardCode::new("PLAIN");
        let state2 = GameStateBuilder::new().with_investigator(inv2).build();
        assert_eq!(super::elder_sign_modifier(&state2, &registry, inv_id2), 0);
    }
}
