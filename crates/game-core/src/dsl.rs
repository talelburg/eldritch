//! Card-effect DSL — v0 primitive set.
//!
//! This module is the alphabet that card declarations speak. A card's
//! abilities are expressed as ([`Trigger`], [`Effect`]) pairs assembled
//! into [`Ability`] values. The engine evaluator (lands when skill
//! tests do, in Phase 3+) walks an [`Effect`] tree to actually mutate
//! game state.
//!
//! # Phase-2 scope
//!
//! Just enough primitives to express the simple Phase-2 cards (Holy
//! Rosary's constant modifiers, Working a Hunch's `on_play` clue
//! discovery). Costs, action abilities, reaction triggers, and
//! complex conditions are deferred — cards needing them get a Rust
//! trait impl until the DSL grows the relevant verbs.
//!
//! # What's not yet expressible
//!
//! Common shapes the DSL cannot describe today, and where they'll
//! land:
//!
//! - **Activated abilities** (`[action]` / `[fast]` symbols on
//!   asset abilities — Hyperawareness's `[fast] Spend 1 resource:
//!   You get +1 [intellect] for this skill test`, etc.). Need a
//!   `Trigger::Activated { action_cost: u8, costs: Vec<Cost> }` plus
//!   cost primitives.
//! - **Forced / leave-play triggers** (Harold Walsted's `Forced — when
//!   Harold Walsted leaves play: Remove him from the game and add...`
//!   from the Dunwich cycle). Need `Trigger::OnLeavePlay` plus
//!   ability-specific effect machinery.
//! - **Reaction abilities** (Roland Banks's `[reaction] After you
//!   defeat an enemy: Discover 1 clue at your location. (Limit once
//!   per round.)`). Need `Trigger::Reaction(EventPattern)` with the
//!   engine's event-window plumbing plus per-round limit tracking.
//! - **Stat-comparison / location-state conditions** (`LocationHasClues`,
//!   `AnyEnemyEngaged`, `SkillSucceededByAtLeast(N)`). Phase-2 only
//!   has [`Condition::SkillTest`] with success/failure granularity.
//!
//! Cards needing any of these go to a Rust impl until the DSL grows
//! the relevant primitive.
//!
//! # Free-function builders
//!
//! Each [`Effect`] variant has a paired free function with a friendly
//! name ([`gain_resources`], [`discover_clue`], etc.). Cards use those
//! to build effect trees readably:
//!
//! ```
//! use game_core::dsl::{constant, modify, ModifierScope, Stat};
//!
//! // Holy Rosary: while in play, +1 willpower.
//! let ability = constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay));
//! ```

use serde::{Deserialize, Serialize};

// ---- triggers --------------------------------------------------

/// When an [`Ability`] is active.
///
/// Phase-3 set. Later phases add `OnEvent(EventPattern)`,
/// `AtPhaseStart`/`AtPhaseEnd`, `OnLeavePlay`, and reaction triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Trigger {
    /// Always-on while the card is in play. The ability's `effect`
    /// describes a passive contribution to engine queries (most
    /// commonly a [`Effect::Modify`] in [`ModifierScope::WhileInPlay`]).
    Constant,
    /// Fires when the card is played out of hand. For events the
    /// effect *is* the card's resolution; for assets it triggers
    /// once at enter-play time (separately from any constant abilities).
    OnPlay,
    /// Fires when the card is committed to a skill test from hand.
    /// Distinct from [`OnPlay`](Self::OnPlay) — commit happens during
    /// any skill test (not just the controller's turn), doesn't cost
    /// resources, and the card discards after the test resolves
    /// rather than entering play. Used by skill cards and a handful
    /// of player cards with commit-time effects (e.g. Deduction's
    /// "if your skill test is successful while investigating, …").
    OnCommit,
    /// Fires when the controller activates the ability via
    /// [`PlayerAction::ActivateAbility`](crate::action::PlayerAction::ActivateAbility).
    ///
    /// `action_cost` mirrors the printed activation cost: `0` for the
    /// `[fast]` symbol (no action), `1` for `[action]`, `N` for multi-
    /// action abilities. Additional costs (resources, exhaust, named-
    /// uses spending) live on [`Ability::costs`], not here — separating
    /// the action-economy cost from arbitrary payment costs keeps
    /// validation and event-emission straightforward.
    Activated {
        /// Number of action points required to activate. `0` = Fast.
        action_cost: u8,
    },
    /// Fires during the resolution of a skill test the card is
    /// committed to, after the outcome is determined and gated on
    /// `outcome` matching it.
    ///
    /// This is not a reaction window (no player decision, no "may");
    /// it's part of the test's own resolution machinery. The effect
    /// evaluates after the action-specific
    /// [`SkillTestFollowUp`](crate::state::SkillTestFollowUp) and
    /// before the committed cards discard, so the source card is
    /// still in hand at evaluation time and
    /// [`LocationTarget::TestedLocation`] resolves against the
    /// in-flight test record.
    ///
    /// Canonical motivating card: Deduction (01039) — "If this skill
    /// test is successful while investigating a location, discover 1
    /// additional clue at that location." See
    /// [issue #112](https://github.com/talelburg/eldritch/issues/112).
    ///
    /// Kind narrowing (the "while investigating" qualifier) is not
    /// baked into the trigger; it's expressed as an [`Effect::If`]
    /// over a kind-aware [`Condition`]. Triggers stay outcome-only
    /// so the surface stays small until a second card with a non-
    /// trivial kind narrowing lands.
    OnSkillTestResolution {
        /// Whether the trigger fires on success or on failure of the
        /// resolving test.
        outcome: TestOutcome,
    },
}

// ---- costs -----------------------------------------------------

/// A payment required to activate an [`Trigger::Activated`] ability.
///
/// All costs on an ability pay together (all-or-nothing) before the
/// ability's effect resolves. The engine validates every cost is
/// payable *before* mutating any state, then pays them in order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Cost {
    /// Spend `n` resources from the controller's wallet. Insufficient
    /// resources reject the activation.
    Resources(u8),
    /// Exhaust the source card. Already-exhausted source rejects.
    /// (Most activated abilities self-exhaust per the rulebook; cards
    /// with a `[fast] no exhaust` ability simply don't list this cost.)
    Exhaust,
    /// Discard a card from the controller's hand. Requires a target
    /// selection via [`AwaitingInput`](crate::EngineOutcome::AwaitingInput)
    /// and a `ResolveInput` dispatch. No card uses this cost yet, so
    /// the engine consumer hasn't landed; activations with this cost
    /// reject with a TODO. Test-side seam is
    /// [`ChoiceResolver`](crate::test_support::ChoiceResolver).
    DiscardCardFromHand,
}

// ---- abilities -------------------------------------------------

/// One ability on a card: a trigger paired with payment costs and
/// the effect that resolves once the costs are paid.
///
/// A card may have multiple [`Ability`] entries — e.g. a constant
/// modifier plus an activated `[fast]` ability.
///
/// `costs` carries any non-action-economy payment (resources, exhaust,
/// named-uses spent) the ability demands. Constant / on-play / on-
/// commit abilities use an empty `costs` vec. Activated abilities
/// list their payment here in addition to the `action_cost` baked
/// into [`Trigger::Activated`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Ability {
    pub trigger: Trigger,
    /// Payment costs (besides action cost). Defaults to empty on
    /// deserialize so older saved logs still load cleanly.
    #[serde(default)]
    pub costs: Vec<Cost>,
    pub effect: Effect,
}

// ---- effects ---------------------------------------------------

/// What an ability does when it resolves.
///
/// Effects compose: [`Effect::Seq`] runs a list in order,
/// [`Effect::If`] branches, [`Effect::ForEach`] applies a body once
/// per resolved target, [`Effect::ChooseOne`] presents alternatives
/// to the controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Effect {
    /// Add resources to the wallet of the resolved target investigator.
    GainResources {
        target: InvestigatorTarget,
        amount: u8,
    },
    /// Move clues from the resolved location to the resolved
    /// investigator. (Caller responsibility: validate the location
    /// has clues and the investigator can hold them.)
    DiscoverClue { from: LocationTarget, count: u8 },
    /// Adjust a stat by `delta` for the duration described by `scope`.
    /// Most scopes are passive contributions to engine queries
    /// rather than mutations of the investigator's stored fields.
    Modify {
        stat: Stat,
        delta: i8,
        scope: ModifierScope,
    },
    /// Run effects in order. Stops at the first non-`Done` outcome
    /// (rejection, awaiting input).
    Seq(Vec<Effect>),
    /// Run `then` if the condition holds at evaluation time, else
    /// `else_` if present.
    If {
        condition: Condition,
        then: Box<Effect>,
        else_: Option<Box<Effect>>,
    },
    /// Resolve `targets` and run `body` once per resolved target.
    /// Each iteration binds the target into the evaluator's scope so
    /// the body can refer to it.
    ForEach {
        targets: InvestigatorTargetSet,
        body: Box<Effect>,
    },
    /// Present alternatives to the controller. Resolves to the chosen
    /// branch's effect. Requires an `AwaitingInput` round-trip; the
    /// evaluator stub for this lands in Phase 3 alongside skill tests.
    ChooseOne(Vec<Effect>),
}

// ---- stats and modifier scopes --------------------------------

/// A statistic that an [`Effect::Modify`] can adjust.
///
/// Phase-2 minimal set: the four skills plus max-health and max-sanity
/// (needed for ally assets like Beat Cop). Action points and other
/// "current" counters get added when cards in later cycles touch them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Stat {
    Willpower,
    Intellect,
    Combat,
    Agility,
    MaxHealth,
    MaxSanity,
}

/// How long an [`Effect::Modify`] applies.
///
/// Phase-3 set. Most cards land in `WhileInPlay` (Holy Rosary's
/// unconditional +1 willpower) or `WhileInPlayDuring(...)` (Magnifying
/// Glass's "+1 intellect *while investigating*" — the qualifier
/// that gates most +stat assets in Core+Dunwich). Commit-time and
/// turn-scoped buffs use `ThisSkillTest` / `ThisTurn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ModifierScope {
    /// Active for as long as the source card is in play. Used by
    /// unqualified constant abilities (Holy Rosary).
    WhileInPlay,
    /// Like [`WhileInPlay`](Self::WhileInPlay) but the modifier only
    /// contributes when the current skill test is of the given kind.
    /// Magnifying Glass's "+1 intellect while investigating" is
    /// `WhileInPlayDuring(SkillTestKind::Investigate)`.
    WhileInPlayDuring(SkillTestKind),
    /// Active until the current skill test resolves. Used by
    /// commit-time bonuses and action abilities like Hyperawareness.
    ThisSkillTest,
    /// Active until the end of the current investigator turn.
    ThisTurn,
}

/// Which kind of skill test is running.
///
/// Cards routinely qualify their bonuses on the test's *kind*, not
/// just the underlying stat — Magnifying Glass's "+1 intellect while
/// investigating" applies to Investigate but **not** to a treachery
/// that tests intellect. Engine-side, every test-initiating action
/// (Investigate, Fight, Evade, the generic
/// [`PerformSkillTest`](crate::action::PlayerAction::PerformSkillTest))
/// passes the matching kind to skill-test resolution.
///
/// Add a variant when a new test-initiating action lands (Parley /
/// Engage will need their own; treacheries that *force* an investigate-
/// flavored test could reuse `Investigate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SkillTestKind {
    /// The Investigate action's intellect test against a location's
    /// shroud.
    Investigate,
    /// The Fight action's combat test against an enemy.
    Fight,
    /// The Evade action's agility test against an enemy.
    Evade,
    /// Any other skill test: treachery effects, agenda effects, or
    /// [`PlayerAction::PerformSkillTest`](crate::action::PlayerAction::PerformSkillTest)
    /// invoked directly. Cards qualifying their bonus with one of the
    /// named-action variants will NOT contribute here.
    Plain,
}

// ---- targets --------------------------------------------------

/// Single-investigator target spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InvestigatorTarget {
    /// The controller of the ability — the investigator who played /
    /// activated this card.
    Controller,
    /// The active investigator at evaluation time. May or may not be
    /// the controller; matters during reactions across turns.
    Active,
    /// The controller picks an investigator. The evaluator presents
    /// the choice via `AwaitingInput`.
    ChosenByController,
}

/// Set-of-investigators target spec for [`Effect::ForEach`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum InvestigatorTargetSet {
    /// All investigators currently in the scenario, in turn order.
    All,
    /// All investigators at the controller's location.
    AtControllerLocation,
}

/// Single-location target spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LocationTarget {
    /// The location the controller is currently at.
    ControllerLocation,
    /// The controller picks a location.
    ChosenByController,
    /// The location associated with the in-flight skill test. For
    /// Investigate that's the location being investigated; the engine
    /// snapshots it onto
    /// [`InFlightSkillTest`](crate::state::InFlightSkillTest) at the
    /// commit window. The
    /// [`OnSkillTestResolution`](Trigger::OnSkillTestResolution)
    /// firing path reads this snapshot. Rejects when
    /// no skill test is in flight or when the snapshotted location
    /// is absent (controller was between locations at test start —
    /// only reachable via [`PerformSkillTest`](crate::action::PlayerAction::PerformSkillTest)
    /// from outside an Investigate path).
    TestedLocation,
}

// ---- conditions -----------------------------------------------

/// A boolean predicate guarding an [`Effect::If`].
///
/// Phase-2 minimal set; later phases will add things like
/// `LocationHasClues`, `AnyEnemyEngaged`, comparisons against
/// stat values, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Condition {
    /// Outcome of the most recent skill test in the current
    /// resolution stack. Deduction's "if successful while
    /// investigating" uses [`TestOutcome::Success`]; failure-triggered
    /// cards (e.g. some Survivor cards) use [`TestOutcome::Failure`].
    SkillTest { outcome: TestOutcome },
}

/// Result of a skill test, as a discrete value usable in conditions.
///
/// For "succeeded by N or more" / "failed by N or more" predicates we
/// can add `SuccessBy(u8)` / `FailureBy(u8)` variants when the first
/// margin-sensitive card lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TestOutcome {
    Success,
    Failure,
}

// ---- builders --------------------------------------------------

/// Construct a [`Trigger::Constant`]-driven [`Ability`] wrapping the
/// given effect. Costs are empty — constant abilities don't pay
/// anything to "fire."
#[must_use]
pub fn constant(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::Constant,
        costs: Vec::new(),
        effect,
    }
}

/// Construct a [`Trigger::OnPlay`]-driven [`Ability`] wrapping the
/// given effect. Costs are empty — the card's play cost (resources to
/// play, action point) is a play-time concern handled elsewhere; the
/// on-play *ability* itself doesn't pay anything additional.
#[must_use]
pub fn on_play(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::OnPlay,
        costs: Vec::new(),
        effect,
    }
}

/// Construct a [`Trigger::OnCommit`]-driven [`Ability`] wrapping the
/// given effect. Used by skill cards and other commit-trigger cards.
#[must_use]
pub fn on_commit(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::OnCommit,
        costs: Vec::new(),
        effect,
    }
}

/// Construct a [`Trigger::OnSkillTestResolution`] ability gated on
/// the given outcome. Costs are empty — resolution-time triggers fire
/// automatically as part of the test's machinery, not via player
/// activation.
#[must_use]
pub fn on_skill_test_resolution(outcome: TestOutcome, effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::OnSkillTestResolution { outcome },
        costs: Vec::new(),
        effect,
    }
}

/// Construct a [`Trigger::Activated`] ability with the given action
/// cost, payment costs, and effect.
///
/// `action_cost`: `0` for `[fast]`, `1` for `[action]`, higher for
/// multi-action abilities.
///
/// `costs`: the non-action payment (resources, exhaust, …). An empty
/// vec is legal — some activated abilities have no payment besides
/// the action cost itself.
#[must_use]
pub fn activated(action_cost: u8, costs: Vec<Cost>, effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::Activated { action_cost },
        costs,
        effect,
    }
}

/// Build an [`Effect::GainResources`].
#[must_use]
pub fn gain_resources(target: InvestigatorTarget, amount: u8) -> Effect {
    Effect::GainResources { target, amount }
}

/// Build an [`Effect::DiscoverClue`].
#[must_use]
pub fn discover_clue(from: LocationTarget, count: u8) -> Effect {
    Effect::DiscoverClue { from, count }
}

/// Build an [`Effect::Modify`].
#[must_use]
pub fn modify(stat: Stat, delta: i8, scope: ModifierScope) -> Effect {
    Effect::Modify { stat, delta, scope }
}

/// Build an [`Effect::Seq`] from any iterable of effects.
///
/// An empty `seq([])` is a no-op when evaluated. Useful as a neutral
/// element in branches (e.g. `if_else(cond, do_thing(), seq([]))`)
/// rather than always providing an `else_` of substance.
#[must_use]
pub fn seq(effects: impl IntoIterator<Item = Effect>) -> Effect {
    Effect::Seq(effects.into_iter().collect())
}

/// Build an [`Effect::If`] with no `else_` branch.
#[must_use]
pub fn if_(condition: Condition, then: Effect) -> Effect {
    Effect::If {
        condition,
        then: Box::new(then),
        else_: None,
    }
}

/// Build an [`Effect::If`] with both branches.
#[must_use]
pub fn if_else(condition: Condition, then: Effect, else_: Effect) -> Effect {
    Effect::If {
        condition,
        then: Box::new(then),
        else_: Some(Box::new(else_)),
    }
}

/// Build an [`Effect::ForEach`].
#[must_use]
pub fn for_each(targets: InvestigatorTargetSet, body: Effect) -> Effect {
    Effect::ForEach {
        targets,
        body: Box::new(body),
    }
}

/// Build an [`Effect::ChooseOne`] from any iterable of effects.
///
/// Empty `choose_one([])` is meaningless — there's nothing to pick —
/// and the evaluator (when it lands in Phase 3) will treat it as a
/// programmer error / log corruption rather than a silent no-op. The
/// DSL doesn't validate emptiness at construction time because card
/// declarations are constants and any card author writing
/// `choose_one([])` is making a typo we want to catch in tests
/// rather than silently swallow.
#[must_use]
pub fn choose_one(effects: impl IntoIterator<Item = Effect>) -> Effect {
    Effect::ChooseOne(effects.into_iter().collect())
}

// ---- tests ----------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Holy Rosary's "while in play, +1 willpower" ability.
    #[test]
    fn holy_rosary_willpower_modifier_compiles() {
        let ability = constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay));
        assert_eq!(ability.trigger, Trigger::Constant);
        assert!(matches!(
            ability.effect,
            Effect::Modify {
                stat: Stat::Willpower,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
            }
        ));
    }

    /// A multi-ability card naturally expressed as two separate
    /// `Ability` declarations: one constant willpower modifier plus a
    /// constant max-health buff. Illustrative shape only — not a real
    /// printed card. (Holy Rosary's `sanity: 2` is *horror-soak*
    /// capacity, NOT a max-sanity modifier; that's a redirect-and-
    /// discard mechanic the DSL doesn't yet model — see #44.)
    #[test]
    fn vec_of_abilities_supports_multiple_constant_modifiers() {
        let abilities = [
            constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay)),
            constant(modify(Stat::MaxHealth, 1, ModifierScope::WhileInPlay)),
        ];
        assert_eq!(abilities.len(), 2);
        assert!(matches!(
            abilities[0].effect,
            Effect::Modify {
                stat: Stat::Willpower,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
            }
        ));
        assert!(matches!(
            abilities[1].effect,
            Effect::Modify {
                stat: Stat::MaxHealth,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
            }
        ));
    }

    /// Working a Hunch's "fast event: discover 1 clue at your location"
    /// — the canonical `OnPlay` + `DiscoverClue` shape.
    #[test]
    fn working_a_hunch_compiles() {
        let ability = on_play(discover_clue(LocationTarget::ControllerLocation, 1));
        assert_eq!(ability.trigger, Trigger::OnPlay);
        assert!(matches!(
            ability.effect,
            Effect::DiscoverClue {
                from: LocationTarget::ControllerLocation,
                count: 1,
            }
        ));
    }

    /// Deduction-shaped commit-trigger ability. The DSL won't fully
    /// resolve this until the engine grows commit-time machinery in
    /// Phase 3, but the type-level construction has to work today so
    /// future commit-trigger cards have somewhere to land.
    #[test]
    fn on_commit_distinct_from_on_play() {
        let ability = on_commit(if_(
            Condition::SkillTest {
                outcome: TestOutcome::Success,
            },
            discover_clue(LocationTarget::ControllerLocation, 1),
        ));
        assert_eq!(ability.trigger, Trigger::OnCommit);
        // Distinct enum variant — compiler enforces the difference at
        // every match site, which is the whole point of separating
        // them rather than reusing OnPlay.
        assert_ne!(ability.trigger, Trigger::OnPlay);
    }

    /// `on_skill_test_resolution` builds the outcome-gated trigger
    /// and accepts `TestedLocation`-targeted effects.
    #[test]
    fn on_skill_test_resolution_builder() {
        let ability = on_skill_test_resolution(
            TestOutcome::Success,
            discover_clue(LocationTarget::TestedLocation, 1),
        );
        assert_eq!(
            ability.trigger,
            Trigger::OnSkillTestResolution {
                outcome: TestOutcome::Success,
            },
        );
        assert!(matches!(
            ability.effect,
            Effect::DiscoverClue {
                from: LocationTarget::TestedLocation,
                count: 1,
            },
        ));
        assert!(ability.costs.is_empty());
    }

    /// Sequence composition: a hypothetical "gain 1 resource AND
    /// discover 1 clue at your location" combined effect.
    #[test]
    fn seq_composition_nests_two_effects() {
        let effect = seq([
            gain_resources(InvestigatorTarget::Controller, 1),
            discover_clue(LocationTarget::ControllerLocation, 1),
        ]);
        match effect {
            Effect::Seq(inner) => assert_eq!(inner.len(), 2),
            _ => panic!("expected Seq"),
        }
    }

    /// `if_` and `if_else` build the same variant; only `else_` differs.
    #[test]
    fn conditional_branches_box_the_inner_effects() {
        let bare = if_(
            Condition::SkillTest {
                outcome: TestOutcome::Success,
            },
            discover_clue(LocationTarget::ControllerLocation, 1),
        );
        let with_else = if_else(
            Condition::SkillTest {
                outcome: TestOutcome::Success,
            },
            discover_clue(LocationTarget::ControllerLocation, 1),
            gain_resources(InvestigatorTarget::Controller, 1),
        );
        assert!(matches!(bare, Effect::If { else_: None, .. }));
        assert!(matches!(with_else, Effect::If { else_: Some(_), .. }));
    }

    /// `for_each` boxes its body and accepts a target-set spec.
    #[test]
    fn for_each_runs_body_per_target() {
        let effect = for_each(
            InvestigatorTargetSet::All,
            gain_resources(InvestigatorTarget::Active, 1),
        );
        assert!(matches!(
            effect,
            Effect::ForEach {
                targets: InvestigatorTargetSet::All,
                ..
            }
        ));
    }

    /// `choose_one` accepts an iterable like `seq`.
    #[test]
    fn choose_one_collects_alternatives() {
        let effect = choose_one([
            gain_resources(InvestigatorTarget::Controller, 2),
            discover_clue(LocationTarget::ControllerLocation, 1),
        ]);
        match effect {
            Effect::ChooseOne(alts) => assert_eq!(alts.len(), 2),
            _ => panic!("expected ChooseOne"),
        }
    }

    /// `InvestigatorTarget::Controller` and `Active` are distinct
    /// variants — they coincide during the controller's own turn but
    /// differ during reactions across turns. The compiler enforces
    /// the difference at every match site; this test pins the
    /// distinction at the type level.
    #[test]
    fn investigator_target_controller_and_active_are_distinct() {
        assert_ne!(InvestigatorTarget::Controller, InvestigatorTarget::Active);
        let controller_effect = gain_resources(InvestigatorTarget::Controller, 1);
        let active_effect = gain_resources(InvestigatorTarget::Active, 1);
        assert_ne!(controller_effect, active_effect);
    }

    /// A deeply-nested effect tree round-trips through `serde_json`.
    /// Cheap insurance against `Box<Effect>` × `#[non_exhaustive]` ×
    /// serde derive surprises.
    #[test]
    fn deeply_nested_effect_round_trips_through_serde_json() {
        let original = seq([
            if_else(
                Condition::SkillTest {
                    outcome: TestOutcome::Success,
                },
                for_each(
                    InvestigatorTargetSet::AtControllerLocation,
                    gain_resources(InvestigatorTarget::Active, 1),
                ),
                modify(Stat::Intellect, -1, ModifierScope::ThisSkillTest),
            ),
            choose_one([
                discover_clue(LocationTarget::ControllerLocation, 1),
                gain_resources(InvestigatorTarget::Controller, 2),
            ]),
        ]);
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    /// Effects clone deeply (the recursive Box doesn't break Clone).
    #[test]
    fn deeply_nested_effect_clones() {
        let original = seq([
            if_else(
                Condition::SkillTest {
                    outcome: TestOutcome::Success,
                },
                for_each(
                    InvestigatorTargetSet::AtControllerLocation,
                    gain_resources(InvestigatorTarget::Active, 1),
                ),
                modify(Stat::Intellect, -1, ModifierScope::ThisSkillTest),
            ),
            choose_one([
                discover_clue(LocationTarget::ControllerLocation, 1),
                gain_resources(InvestigatorTarget::Controller, 2),
            ]),
        ]);
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }
}
