//! Card-effect DSL — v0 primitive set.
//!
//! This module is the alphabet that card declarations speak. A card's
//! abilities are expressed as ([`Trigger`], [`Effect`]) pairs assembled
//! into [`Ability`] values. The engine evaluator (lands when skill
//! tests do, in Phase 3+) walks an [`Effect`] tree to actually mutate
//! game state.
//!
//! # Scope
//!
//! The primitive set grows as cards demand. Today the DSL covers
//! constant modifiers, on-play and on-commit triggers, activated
//! abilities with action / payment costs, a skill-test-resolution
//! trigger, and a revelation trigger for encounter-card-reveal effects.
//! Reaction-style abilities have a DSL surface
//! ([`Trigger::OnEvent`]) but no engine machinery yet (see below).
//! Cards needing primitives the DSL doesn't yet express get a Rust
//! trait impl until the verb lands.
//!
//! # What's not yet expressible
//!
//! Common shapes the DSL cannot describe today, and where they'll
//! land:
//!
//! - **Forced / leave-play triggers** (Harold Walsted's `Forced — when
//!   Harold Walsted leaves play: Remove him from the game and add...`
//!   from the Dunwich cycle). Need `Trigger::OnLeavePlay` plus
//!   ability-specific effect machinery.
//! - **Stat-comparison / location-state conditions** (`LocationHasClues`,
//!   `AnyEnemyEngaged`, `SkillSucceededByAtLeast(N)`). [`Condition`]
//!   today only covers skill-test outcome and kind.
//!
//! # Has DSL surface but not yet engine support
//!
//! - **Reaction abilities** (Roland Banks's `[reaction] After you
//!   defeat an enemy: Discover 1 clue at your location. (Limit once
//!   per round.)`). [`Trigger::OnEvent`] compiles and round-trips
//!   through serde, but the engine event-window plumbing —
//!   registering active triggers from cards in play and firing them
//!   against emitted events — lands in
//!   [issue #52](https://github.com/talelburg/eldritch/issues/52),
//!   and per-round limit tracking still needs a primitive.
//!
//! Cards needing primitives in either list go to a Rust impl until
//! the relevant verb lands.
//!
//! # Free-function builders
//!
//! Each [`Effect`] variant has a paired free function with a friendly
//! name ([`gain_resources`], [`discover_clue`], etc.). Cards use those
//! to build effect trees readably:
//!
//! ```
//! use card_dsl::{constant, modify, ModifierScope, Stat};
//!
//! // Holy Rosary: while in play, +1 willpower.
//! let ability = constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay));
//! ```

use serde::{Deserialize, Serialize};

// ---- triggers --------------------------------------------------

/// When an [`Ability`] is active.
///
/// Phase-3 set. Later phases add `AtPhaseStart`/`AtPhaseEnd`,
/// `OnLeavePlay`, and additional reactive patterns as cards demand.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Fires when the owning card is revealed from the encounter deck.
    ///
    /// First consumer: the synthetic treachery in
    /// `scenarios::test_fixtures::synth_cards`. Real Phase-7+ treachery
    /// cards will replace the synthetic fixture's role as primary
    /// consumer.
    ///
    /// Distinct from [`OnPlay`](Self::OnPlay) — Revelation fires for engine-driven
    /// encounter draws (Mythos phase, scenario forced effects), not
    /// for cards played from a player's hand. Treacheries are never
    /// in a player's hand; they're encounter-bag content.
    ///
    /// The engine's on-draw resolution path (`encounter_card_revealed`
    /// in `game-core`'s `engine::dispatch`) runs every
    /// `Trigger::Revelation` ability on the drawn card through the DSL
    /// evaluator, then discards the treachery (or hands off to the
    /// spawn handler for enemies — landing in #127).
    Revelation,
    /// Fires when the controller activates the ability via
    /// `PlayerAction::ActivateAbility` (in `game_core::action`).
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
    /// `SkillTestFollowUp` (in `game_core::state`) and
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
    ///
    /// Distinct from the after-resolution reactive trigger window
    /// tracked in [issue #64](https://github.com/talelburg/eldritch/issues/64),
    /// which fires *after* the test ends with a player decision
    /// window ("after a test succeeds, you may …"). This trigger
    /// runs as part of the test's resolution machinery with no
    /// player choice; route card text by which timing fits.
    OnSkillTestResolution {
        /// Whether the trigger fires on success or on failure of the
        /// resolving test.
        outcome: TestOutcome,
    },
    /// Fires when an engine `Event` (in `game_core`) matching `pattern`
    /// is emitted, in the reaction window opened by the engine for
    /// the corresponding `timing`.
    ///
    /// Canonical motivating card: Roland Banks (01001) —
    /// `[reaction] After you defeat an enemy: Discover 1 clue at your
    /// location.` compiles to `OnEvent { pattern: EnemyDefeated {
    /// by_controller: true }, timing: After }`.
    ///
    /// Distinct from [`OnSkillTestResolution`](Self::OnSkillTestResolution),
    /// which fires inside a skill test's own resolution machinery
    /// (no player decision, no `may`). `OnEvent` triggers fire in
    /// reaction windows where the controller may choose to use them.
    ///
    /// The DSL surface lands here; the engine machinery that
    /// registers these triggers from cards in play and fires them
    /// during reaction windows lands in
    /// [issue #52](https://github.com/talelburg/eldritch/issues/52).
    /// Until then the engine ignores `OnEvent` abilities; cards
    /// declaring one compile and round-trip through serde but
    /// otherwise do nothing at runtime.
    OnEvent {
        /// Which engine event(s) trigger this ability.
        pattern: EventPattern,
        /// Whether the trigger fires before or after the matching
        /// event finalizes.
        timing: EventTiming,
        /// Whether this is a mandatory **forced** ability or an optional
        /// player **reaction**. Determines which phase of the two-phase
        /// `emit_event` dispatch it participates in — Rules Reference p.2:
        /// "all forced abilities … must resolve before any `[reaction]`
        /// abilities … may be initiated." Replaces the earlier
        /// route-by-`EventPattern` heuristic (which forced twin patterns
        /// for one game moment, e.g. `AfterLocationInvestigated` forced
        /// vs `SuccessfullyInvestigated` reaction).
        kind: TriggerKind,
    },
}

/// Whether an [`Trigger::OnEvent`] ability resolves mandatorily (forced)
/// or is an optional player reaction.
///
/// Forced abilities all resolve before any reaction abilities at the same
/// timing point (Rules Reference p.2), and the engine's `emit_event`
/// dispatch keys its two phases off this distinction rather than guessing
/// from the [`EventPattern`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerKind {
    /// Mandatory; resolves automatically (the player only orders
    /// simultaneous ones). Phase 1 of `emit_event`.
    Forced,
    /// Optional; the controller may use it in the reaction window.
    /// Phase 2 of `emit_event`.
    Reaction,
}

/// Which engine event(s) an [`Trigger::OnEvent`] ability listens for.
///
/// Phase-3 minimal set: just the variant Roland Banks needs. Grows as
/// later cards demand new patterns (skill-test outcomes, investigator
/// movement, clue placement, …); the engine evaluator exhaustively
/// matches on this enum so adding a variant is a deliberate change
/// rather than a silent broadening.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventPattern {
    /// An enemy was defeated. `by_controller` narrows the match to
    /// defeats credited to the ability's controller — Roland Banks's
    /// "after **you** defeat an enemy" sets this `true`; an
    /// unqualified "after an enemy is defeated" would set it `false`.
    EnemyDefeated {
        /// If `true`, only fires when the controller of this ability
        /// is credited with the defeat (the `by` field of
        /// `game_core::Event::EnemyDefeated`). If `false`, any defeat
        /// matches.
        by_controller: bool,
        /// Narrow the match to a specific defeated enemy printed code
        /// (e.g. the Ghoul Priest's `"01116"` for Act 3's objective).
        /// `None` matches any enemy's defeat (e.g. Roland's reaction).
        code: Option<String>,
    },
    /// An encounter card was revealed (drawn from the encounter deck
    /// and announced via the engine's on-draw path). `card_type`
    /// narrows the match: `None` matches any reveal, `Some(card_type)`
    /// matches only reveals whose card type equals the given value.
    ///
    /// Canonical listener shape: a hypothetical Forewarned-style
    /// cancellation effect would set `card_type: Some(CardType::Treachery)`
    /// to react only to treachery reveals. No card uses this pattern in
    /// the Phase-4 scope; the DSL surface lands here, the engine's
    /// reaction-window machinery (#52) fires it.
    ///
    /// **Why `card_type` not `by_controller`:** encounter draws are
    /// engine-driven, not card-controlled. The `EnemyDefeated`-style
    /// `by_controller: bool` qualifier doesn't fit. Treachery-vs-enemy
    /// narrowing is the load-bearing distinction for hypothetical
    /// listener cards instead.
    CardRevealed {
        /// Narrow the match by card type. `None` = any reveal.
        card_type: Option<crate::card_data::CardType>,
    },
    /// An enemy spawned at a location (entered play from the
    /// encounter deck via the on-draw resolution path).
    ///
    /// Intentionally bare (no narrowing fields). YAGNI on
    /// `by_controller` / `card_type` / `location_filter` until a
    /// real listener forces a shape. Concrete-consumer-first.
    ///
    /// First listener will likely be a Phase-7+ "after an enemy
    /// spawns at your location" reaction; that PR gets to extend
    /// this variant with whatever narrowing field it needs.
    EnemySpawned,
    /// An investigator entered the location this ability is printed on
    /// (Forced "after you enter \<location\>" effects: Attic `01113`
    /// takes 1 horror, Cellar `01114` takes 1 damage).
    ///
    /// Intentionally bare: the engine binds *you* = the entering
    /// investigator and *this location* = the ability's own location
    /// from the trigger context — no narrowing fields needed.
    ///
    /// The forced dispatch path matches this pattern and fires it from
    /// `move_action` on entry (`engine::dispatch::forced_triggers`).
    EnteredLocation,
    /// A game phase ended. Forced agenda/act effects keyed to a phase
    /// boundary listen here: agenda `01107` moves Ghouls at
    /// `PhaseEnded { phase: Enemy }`. (Its end-of-round doom keys off
    /// [`RoundEnded`](Self::RoundEnded), not `PhaseEnded { Upkeep }`.)
    ///
    /// Matched only by the forced dispatch path
    /// (`engine::dispatch::forced_triggers`), never by player reaction
    /// windows — `trigger_matches` returns `false` for it. Currently
    /// wired for Enemy and Upkeep phase-ends only; Mythos and
    /// Investigation are not wired (see #212).
    PhaseEnded { phase: Phase },
    /// The act this ability is printed on advanced (its reverse side
    /// resolves). Fired forced via `ForcedTriggerPoint::ActAdvanced`;
    /// binds controller = the lead investigator (board-wide reverse
    /// effects ignore it).
    ActAdvanced,
    /// The agenda this ability is printed on advanced (its reverse side
    /// resolves on doom). Fired forced via
    /// `ForcedTriggerPoint::AgendaAdvanced` from `advance_agenda` (the
    /// mirror of the act path — `advance_act` fires `ActAdvanced`); binds
    /// controller = the lead investigator. The Gathering's agenda reverses
    /// listen here: 01105 (lead's discard/horror choice) and 01106
    /// (dig the encounter deck until a `Ghoul` enemy, lead draws it).
    AgendaAdvanced,
    /// The round ended (Rules Reference p.24: the round ends at the close
    /// of the upkeep phase). Forced agenda/act effects keyed to "at the
    /// end of the round" listen here — agenda `01107` places doom. Fired
    /// forced via `ForcedTriggerPoint::RoundEnded`; binds controller =
    /// the lead investigator (board-wide effects ignore it). Distinct
    /// from `PhaseEnded { Upkeep }` so an "end of upkeep phase" and an
    /// "end of round" card can coexist.
    RoundEnded,
    /// The investigator's turn ended (Rules Reference p.24 step 2.2.2,
    /// "Forced – At the end of your turn"). Fired forced via
    /// `ForcedTriggerPoint::EndOfTurn` from `end_turn`, scanning the
    /// ending investigator's controlled card instances (threat area +
    /// in play); binds controller = that investigator. First consumer:
    /// Frozen in Fear (01164), C4c (#235).
    EndOfTurn,
    /// A location was successfully investigated. Fired forced via
    /// `ForcedTriggerPoint::AfterLocationInvestigated` from the
    /// skill-test resolution driver after a successful Investigate;
    /// binds controller = the investigating investigator. In C4a the
    /// forced scan covers the investigator's controlled card instances;
    /// C4c (#235) extends it to the investigated location's attachment
    /// zone for Obscuring Fog (01168), the first consumer.
    AfterLocationInvestigated,
    /// An investigator is about to discover one or more clues. Matched
    /// **only** by the clue-discovery interrupt seam in `discover_clue`
    /// (paired with [`EventTiming::Before`]), never by the general
    /// reaction-window pipeline — `trigger_matches` returns `false` for
    /// it, like the forced-only patterns above. First consumer: Cover Up
    /// 01007's "`[reaction]` When you would discover 1 or more clues at your
    /// location: Discard that many clues from Cover Up instead." (C5a #236.)
    WouldDiscoverClues,
    /// The game ended (a scenario resolution latched). Fired forced via
    /// `ForcedTriggerPoint::GameEnd` from `fire_scenario_resolution`,
    /// scanning every investigator's controlled card instances; binds
    /// controller = each instance's controller. First consumer: Cover Up
    /// 01007's "Forced - When the game ends, if there are any clues on
    /// Cover Up: You suffer 1 mental trauma." (C5a #236.)
    GameEnd,
    /// You successfully investigated — the **player-reaction** timing of
    /// "`[reaction]` After you successfully investigate" (Dr. Milan
    /// Christopher 01033: gain 1 resource). Bare: the engine binds *you* =
    /// the investigating investigator from the window context.
    ///
    /// Distinct from [`AfterLocationInvestigated`](Self::AfterLocationInvestigated),
    /// the **forced** twin of the same Arkham timing (Obscuring Fog 01168).
    /// They are separate patterns only because this codebase has no
    /// `Trigger::Forced`: the engine routes by pattern, firing
    /// `AfterLocationInvestigated` through the forced auto-fire path and
    /// `SuccessfullyInvestigated` through a player reaction window
    /// (`WindowKind::AfterSuccessfulInvestigate`). Unifying forced +
    /// reaction at one window is the #212/#213 trigger-dispatch work; until
    /// then the split pattern keeps a forced ability from auto-firing a
    /// reaction (and vice versa).
    SuccessfullyInvestigated,
    /// An enemy attack dealt damage to the asset this ability is printed
    /// on (the soaked ally). Bare — the engine binds *self* = the soaked
    /// asset instance from the firing window context, the way
    /// [`EnteredLocation`](Self::EnteredLocation) / [`EndOfTurn`](Self::EndOfTurn)
    /// bind theirs. Matched **only** by
    /// `WindowKind::AfterEnemyAttackDamagedAsset` in the reaction
    /// pipeline; `trigger_matches` binds the attacking enemy into the
    /// `EvalContext`. First (and only) consumer: Guard Dog 01021's
    /// "\[reaction\] When an enemy attack deals damage to Guard Dog: Deal 1
    /// damage to the attacking enemy." (C5b #237.)
    EnemyAttackDamagedSelf,
}

/// The four game phases, mirrored in `card-dsl` so [`EventPattern`] can
/// name a phase without `card-dsl` depending on `game-core` (layering).
/// `game-core` maps this to its own `state::Phase` at the dispatch
/// boundary (see `engine::dispatch::forced_triggers`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    Mythos,
    Investigation,
    Enemy,
    Upkeep,
}

/// When an [`Trigger::OnEvent`] ability fires relative to the
/// triggering event finalizing.
///
/// Most reaction cards use [`After`](Self::After) ("After you defeat
/// an enemy …"). [`Before`](Self::Before) is the "Forced — when …
/// would …" timing that lets an effect interpose on an in-progress
/// event; no card uses it in the Phase-3 scope yet, but the variant
/// is included so #52's reaction-window machinery can hang both
/// windows off the same trigger surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventTiming {
    /// Resolves before the triggering event finalizes.
    Before,
    /// Resolves after the triggering event has finalized.
    After,
}

// ---- costs -----------------------------------------------------

/// A payment required to activate an [`Trigger::Activated`] ability.
///
/// All costs on an ability pay together (all-or-nothing) before the
/// ability's effect resolves. The engine validates every cost is
/// payable *before* mutating any state, then pays them in order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cost {
    /// Spend `n` resources from the controller's wallet. Insufficient
    /// resources reject the activation.
    Resources(u8),
    /// Exhaust the source card. Already-exhausted source rejects.
    /// (Most activated abilities self-exhaust per the rulebook; cards
    /// with a `[fast] no exhaust` ability simply don't list this cost.)
    Exhaust,
    /// Discard a card from the controller's hand. Requires a target
    /// selection via `AwaitingInput` (the `game_core::EngineOutcome` variant)
    /// and a `ResolveInput` dispatch. No card uses this cost yet, so
    /// the engine consumer hasn't landed; activations with this cost
    /// reject with a TODO. Test-side seam is
    /// `ChoiceResolver` (in `game_core::test_support`).
    DiscardCardFromHand,
    /// Spend `count` tokens of the named [`UseKind`](crate::card_data::UseKind)
    /// from the source asset's runtime uses-pool (".38 Special": "Spend 1
    /// ammo"). Insufficient remaining of that kind rejects the activation.
    SpendUses {
        /// Which uses-kind to spend (Ammo, Charges, …).
        kind: crate::card_data::UseKind,
        /// How many to spend.
        count: u8,
    },
}

// ---- usage limits ----------------------------------------------

/// A "Limit X per \[period\]" cap on how often an ability may fire.
///
/// Per the Rules Reference page 14: *"Each instance of an ability with
/// such a limit may be initiated X times during the designated period.
/// If a card leaves play and re-enters play during the same period,
/// the card is considered to be bringing a new instance of the ability
/// to the game."*
///
/// Canonical motivating card: Roland Banks (01001) —
/// `[reaction] After you defeat an enemy: Discover 1 clue at your
/// location. (Limit once per round.)` compiles to
/// `UsageLimit { count: 1, period: UsagePeriod::Round }`.
///
/// Storage of the per-instance counter lives on
/// `CardInPlay` (in `game_core::state`): see
/// `ability_usage` (its per-instance counter map). When a
/// card leaves play, its `CardInPlay` is dropped, so a re-entering
/// instance starts fresh as the rules require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UsageLimit {
    /// Maximum number of times the ability may fire during one period.
    pub count: u8,
    /// Which period the count is measured over.
    pub period: UsagePeriod,
}

/// The period a [`UsageLimit`] is measured over.
///
/// Phase-3 minimal set: `Round` (Roland's "Limit once per round").
/// `Phase` ("limit once per turn") and `Game` ("limit once per game"
/// — group or player) land when the first consumer appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsagePeriod {
    /// A game round, as defined by the framework: begins at 1.1 Mythos,
    /// ends at 4.6 Upkeep (Rules Reference page 23). Counter resets
    /// when `GameState::round` (in `game_core::state`)
    /// advances.
    Round,
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
///
/// `usage_limit` carries the "Limit X per period" cap on firing — see
/// [`UsageLimit`]. `None` means "unlimited within the rules' default
/// once-per-occurrence cap on reaction abilities" (Rules Reference
/// page 2). A `Some(...)` value applies the stronger printed cap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Ability {
    pub trigger: Trigger,
    /// Payment costs (besides action cost). Defaults to empty on
    /// deserialize so older saved logs still load cleanly.
    #[serde(default)]
    pub costs: Vec<Cost>,
    pub effect: Effect,
    /// "Limit X per \[period\]" cap. `None` for abilities with no
    /// printed cap. Defaults to `None` on deserialize so older saved
    /// logs still load cleanly.
    #[serde(default)]
    pub usage_limit: Option<UsageLimit>,
}

impl Ability {
    /// Attach a [`UsageLimit`] to this ability. Builder-style sugar so
    /// card impls can chain off the `on_event(...)` / `activated(...)`
    /// constructors instead of mutating fields by name (which the
    /// `cards` crate can't do anyway — [`Ability`] is
    /// `#[non_exhaustive]`).
    #[must_use]
    pub fn with_usage_limit(mut self, limit: UsageLimit) -> Self {
        self.usage_limit = Some(limit);
        self
    }
}

// ---- effects ---------------------------------------------------

/// What an ability does when it resolves.
///
/// Effects compose: [`Effect::Seq`] runs a list in order,
/// [`Effect::If`] branches, [`Effect::ForEach`] applies a body once
/// per resolved target, [`Effect::ChooseOne`] presents alternatives
/// to the controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Deal `amount` damage to the resolved target investigator,
    /// applying defeat if the new total reaches their max health.
    /// `amount == 0` is a no-op (no event, no target resolution).
    DealDamage {
        target: InvestigatorTarget,
        amount: u8,
    },
    /// Deal `amount` horror to the resolved target investigator,
    /// applying defeat if the new total reaches their max sanity.
    /// `amount == 0` is a no-op.
    DealHorror {
        target: InvestigatorTarget,
        amount: u8,
    },
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
    /// Advance the current act one step. If the act is terminal (carries
    /// a resolution) the scenario resolves; otherwise the cursor moves
    /// and the act's on-advance reverse fires. Used by act objectives
    /// like 01110 ("If the Ghoul Priest is Defeated, advance.").
    AdvanceCurrentAct,
    /// A card-local Rust effect, resolved by tag through the host's
    /// `CardRegistry.native_effect_for`. The generic escape hatch for
    /// single-use card logic that doesn't earn a shared `Effect` variant
    /// (see issue #276). The `cards` crate maps the tag to a Rust fn; the
    /// evaluator rejects loudly on an unknown tag or absent registry.
    Native { tag: String },
    /// Initiate a skill test as part of a card effect (treachery
    /// Revelation, agenda forced effect, …). The evaluator maps `skill`
    /// to the engine's `SkillKind` and runs the test against
    /// `difficulty`, always suspending at the commit window. `on_fail`
    /// runs after the test resolves **on failure**, with the failure
    /// margin available via the evaluator context's `failed_by` (success
    /// is a no-op for the cards in scope). See issue #286.
    SkillTest {
        skill: crate::card_data::SkillKind,
        difficulty: u8,
        /// Effect to run **on success** after the test resolves. Frozen in
        /// Fear 01164 discards itself on a successful end-of-turn willpower
        /// test. `None` for tests with no success-side effect.
        on_success: Option<Box<Effect>>,
        /// Effect to run **on failure** after the test resolves, with the
        /// failure margin available via the evaluator context's `failed_by`.
        /// `None` for tests with no failure-side effect. Symmetric with
        /// `on_success` — success and margin-keyed-failure are separate axes.
        on_fail: Option<Box<Effect>>,
    },
    /// Run `body` once per point the just-resolved skill test was failed
    /// by ("for each point you fail by, …"). Reads the failure margin
    /// from the evaluator context; a `0` margin (or no margin in context)
    /// runs `body` zero times. Only meaningful inside an
    /// [`Effect::SkillTest`]'s `on_fail`.
    ForEachPointFailed(Box<Effect>),
    /// Discard the firing card instance (the evaluator context's
    /// `source`). Locates the instance in a threat area or location
    /// attachment, removes it, and discards it to the encounter discard.
    /// Used by persistent treacheries' `Forced` self-discard abilities
    /// (Frozen in Fear 01164, Dissonant Voices 01165, Obscuring Fog
    /// 01168). Rejects if there is no source or the instance isn't found.
    DiscardSelf,
    /// Put the card with this printed `code` into the controller's threat
    /// area as a fresh in-play instance. The Revelation of persistent
    /// threat-area treacheries (Frozen in Fear 01164, Dissonant Voices
    /// 01165) — the card names its own `CODE`. (Attaching to a *location*
    /// stays card-local because of per-card rules like Obscuring Fog's
    /// "Limit 1 per location".)
    ///
    /// The `code` is carried because at Revelation the card isn't in play
    /// yet, so the evaluator context has no instance handle for "self"
    /// (unlike [`DiscardSelf`](Self::DiscardSelf), which reads the
    /// already-in-play `EvalContext.source`). `TODO(#290)`: once encounter
    /// cards are minted as in-play instances *at reveal* (so the source
    /// instance exists before the Revelation runs), this can drop the
    /// `code` and place "self" uniformly with `DiscardSelf`.
    PutIntoThreatArea {
        /// Printed `ArkhamDB` code of the card to place.
        code: String,
        /// Clues to seed on the placed instance ("with 3 clues on it",
        /// Cover Up 01007). `0` for cards that enter clue-less.
        clues: u8,
    },
    /// Initiate a Fight against the single enemy engaged with the
    /// controller, applying `combat_modifier` (resolved at eval, e.g.
    /// .38 Special's +1/+3) for this attack and dealing `1 + extra_damage`
    /// on success. Auto-targets when exactly one enemy is engaged; the
    /// activation check rejects ≠1 engaged *before* any cost is paid, so
    /// the evaluator can assume a single target. Inspectable (not
    /// `Native`) precisely so that pre-charge target check can see it.
    Fight {
        /// Combat modifier for this attack, resolved against state at eval.
        combat_modifier: IntExpr,
        /// Bonus damage beyond the base 1 (.38 Special: +1).
        extra_damage: u8,
    },
    /// Draw `count` cards for the resolved target investigator —
    /// "draw 1 card" (Guts 01089, Perception 01090, Overpower 01091,
    /// Manual Dexterity 01092). `count == 0` is a no-op. Deck-out (drawing
    /// from an empty deck) follows the engine's existing `draw_cards`
    /// behavior; the elimination consequence is out of this primitive's
    /// scope.
    DrawCards {
        target: InvestigatorTarget,
        count: u8,
    },
    /// Add `N` to the in-flight skill test's bonus attack damage —
    /// Vicious Blow 01025's "that attack deals +1 damage." Accumulated at
    /// commit time (under [`Trigger::OnCommit`]) onto the in-flight
    /// record; **only a Fight skill test's follow-up reads it**, so the
    /// "during an attack" qualifier is intrinsic (committing to a
    /// non-attack test accumulates harmlessly and changes nothing), as is
    /// "if successful" (the Fight follow-up deals damage only on success).
    /// A no-op when there is no in-flight test.
    BoostAttackDamage(u8),
    /// A constant restriction the source card imposes while in play
    /// (under [`Trigger::Constant`]). **Inspected, not executed** — the
    /// engine reads it at the relevant decision point (`play_is_prohibited`
    /// for `CannotPlay`, `pending_action_surcharge` for `ExtraActionCost`);
    /// resolving it as an effect is a misuse and rejects.
    Restrict(Restriction),
}

// ---- stats and modifier scopes --------------------------------

/// A statistic that an [`Effect::Modify`] can adjust.
///
/// Phase-2 minimal set: the four skills plus max-health and max-sanity
/// (needed for ally assets like Beat Cop). Action points and other
/// "current" counters get added when cards in later cycles touch them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stat {
    Willpower,
    Intellect,
    Combat,
    Agility,
    MaxHealth,
    MaxSanity,
    /// A location's shroud (investigate difficulty), adjusted by
    /// location attachments such as Obscuring Fog 01168's `+2`.
    Shroud,
}

/// How long an [`Effect::Modify`] applies.
///
/// Phase-3 set. Most cards land in `WhileInPlay` (Holy Rosary's
/// unconditional +1 willpower) or `WhileInPlayDuring(...)` (Magnifying
/// Glass's "+1 intellect *while investigating*" — the qualifier
/// that gates most +stat assets in Core+Dunwich). Commit-time and
/// turn-scoped buffs use `ThisSkillTest` / `ThisTurn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// A constant restriction a card imposes while in play, carried by a
/// [`Trigger::Constant`] [`Effect::Restrict`]. The engine inspects these
/// at decision points rather than executing them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Restriction {
    /// The controller cannot play cards of this type (Dissonant Voices
    /// 01165 declares one per forbidden type — assets and events).
    CannotPlay(crate::card_data::CardType),
    /// Performing one of `actions` costs 1 additional action. When
    /// `first_each_round` is set, only the first matching action each
    /// round is surcharged (Frozen in Fear 01164).
    ///
    /// TODO: the `first_each_round` gate also applies to non-cost
    /// mechanisms (a constant ability that suppresses attacks of
    /// opportunity on the first action each round; a forced trigger on the
    /// first move each turn). Promote it to a shared "first-applicable each
    /// round/turn" scope spanning constant modifiers and forced triggers
    /// once a second mechanism needs the same gate — not while action cost
    /// is its only consumer.
    ExtraActionCost {
        /// Which action kinds are surcharged (Frozen in Fear 01164: move,
        /// fight, evade).
        actions: Vec<ActionClass>,
        /// Gate the surcharge to the first matching action each round.
        first_each_round: bool,
    },
}

/// One action kind an [`Restriction::ExtraActionCost`] can surcharge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionClass {
    /// The Move action.
    Move,
    /// The Fight action.
    Fight,
    /// The Evade action.
    Evade,
}

/// Which kind of skill test is running.
///
/// Cards routinely qualify their bonuses on the test's *kind*, not
/// just the underlying stat — Magnifying Glass's "+1 intellect while
/// investigating" applies to Investigate but **not** to a treachery
/// that tests intellect. Engine-side, every test-initiating action
/// (Investigate, Fight, Evade, the generic
/// `PerformSkillTest` (in `game_core::action::PlayerAction`))
/// passes the matching kind to skill-test resolution.
///
/// Add a variant when a new test-initiating action lands (Parley /
/// Engage will need their own; treacheries that *force* an investigate-
/// flavored test could reuse `Investigate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillTestKind {
    /// The Investigate action's intellect test against a location's
    /// shroud.
    Investigate,
    /// The Fight action's combat test against an enemy.
    Fight,
    /// The Evade action's agility test against an enemy.
    Evade,
    /// Any other skill test: treachery effects, agenda effects, or
    /// `PlayerAction::PerformSkillTest` (in `game_core::action`)
    /// invoked directly. Cards qualifying their bonus with one of the
    /// named-action variants will NOT contribute here.
    Plain,
}

// ---- targets --------------------------------------------------

/// A controller-facing choice of a board entity or location. Generic over its
/// `scope` (the candidate filter). `chooser` is deferred — every choice is the
/// controller's today; agenda 01105's "lead" choice already works via the
/// forced-dispatch `controller = lead` binding. The wrapper reserves its home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Choose<S> {
    /// The candidate filter (an [`EntityScope`] or [`LocationSet`]).
    pub scope: S,
}

/// The chooser-relative set of locations a choice is measured against — shared
/// by location-picks (which locations may I pick?) and entity-position filters
/// (where must the entity be?), so "your location" is defined once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LocationSet {
    /// The chooser's own location ("your location"). Empty when the chooser is
    /// between locations.
    Here,
    /// Any location in play (the old bare `ChosenByController` for locations).
    Anywhere,
    // `YourOrConnecting` is added by PR-8 (#306) with the adjacency model.
}

/// An entity-choice filter. Locational today; non-spatial arms (`Engaged`,
/// `WithTrait`, …) accrete here when a card needs them — additively, touching
/// neither [`LocationSet`] nor location-picks. (The `UsagePeriod::Round`-only
/// minimal-enum-with-a-growth-path idiom.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityScope {
    /// An entity whose location is in the given [`LocationSet`].
    At(LocationSet),
}

/// Single-investigator target spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InvestigatorTarget {
    /// The investigator this ability acts on — "you" in card text. For
    /// a played/activated card that's whoever played it; for a forced
    /// trigger it's the affected investigator the dispatcher binds (e.g.
    /// the one entering a location for an "After you enter" effect).
    You,
    /// The active investigator at evaluation time. May or may not be
    /// "you"; matters during reactions across turns.
    Active,
    /// The chooser picks one investigator from the [`Choose`]'s scope. Bound by
    /// the evaluator's `ground_chosen_targets` before the effect's handler runs.
    Chosen(Choose<EntityScope>),
}

impl InvestigatorTarget {
    /// "Choose an investigator" with no location constraint (any investigator
    /// in play). The successor to the bare `ChosenByController`.
    #[must_use]
    pub fn chosen_anywhere() -> Self {
        InvestigatorTarget::Chosen(Choose {
            scope: EntityScope::At(LocationSet::Anywhere),
        })
    }
}

/// Set-of-investigators target spec for [`Effect::ForEach`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InvestigatorTargetSet {
    /// All investigators currently in the scenario, in turn order.
    All,
    /// All investigators at the controller's location.
    AtControllerLocation,
}

/// Single-location target spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LocationTarget {
    /// The location "you" are currently at — the location of the
    /// investigator this ability acts on (see [`InvestigatorTarget::You`]).
    YourLocation,
    /// The chooser picks one location from the [`Choose`]'s scope. Bound by
    /// `ground_chosen_targets` before the handler runs.
    Chosen(Choose<LocationSet>),
    /// The location associated with the in-flight skill test. For
    /// Investigate that's the location being investigated; the engine
    /// snapshots it onto
    /// `InFlightSkillTest` (in `game_core::state`) at the
    /// commit window. The
    /// [`OnSkillTestResolution`](Trigger::OnSkillTestResolution)
    /// firing path reads this snapshot. Rejects when
    /// no skill test is in flight or when the snapshotted location
    /// is absent (controller was between locations at test start —
    /// only reachable via `PlayerAction::PerformSkillTest` (in `game_core::action`)
    /// from outside an Investigate path).
    TestedLocation,
}

impl LocationTarget {
    /// "Choose a location" with no constraint (any location in play). The
    /// successor to the bare `ChosenByController`.
    #[must_use]
    pub fn chosen_anywhere() -> Self {
        LocationTarget::Chosen(Choose {
            scope: LocationSet::Anywhere,
        })
    }
}

// ---- conditions -----------------------------------------------

/// A boolean predicate guarding an [`Effect::If`].
///
/// Phase-2 minimal set; later phases will add things like
/// `LocationHasClues`, `AnyEnemyEngaged`, comparisons against
/// stat values, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Condition {
    /// Outcome of the most recent skill test in the current
    /// resolution stack. Failure-triggered cards (some Survivor
    /// cards) use [`TestOutcome::Failure`]; success-side card text
    /// typically gates via [`Trigger::OnSkillTestResolution`] instead,
    /// so this variant is rarely paired with `Success`.
    SkillTest { outcome: TestOutcome },
    /// Kind of the currently-resolving skill test. Used to narrow
    /// effects whose printed text qualifies on the action that
    /// initiated the test — Deduction's "if this skill test is
    /// successful **while investigating** …" wraps its bonus-clue
    /// effect in `If(SkillTestKind(Investigate), …)`. Holds when
    /// there's an in-flight test whose kind matches; rejects when
    /// no test is in flight.
    SkillTestKind(SkillTestKind),
    /// Holds when the controller's current location has ≥1 clue.
    /// ".38 Special": "if there are 1 or more clues on your location".
    LocationHasClues,
}

/// An integer computed at effect-evaluation time. Lets a numeric field
/// carry a condition-gated value without duplicating the surrounding
/// effect — ".38 Special" reads its combat modifier as
/// `IntExpr::cond(LocationHasClues, 3, 1)` rather than an
/// [`Effect::If`] wrapping two near-identical `fight(…)` nodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntExpr {
    /// A literal value.
    Lit(i8),
    /// `then` if `when` holds at eval time, else `otherwise`.
    Cond {
        /// Predicate evaluated against current state.
        when: Condition,
        /// Value when the predicate holds.
        then: i8,
        /// Value when it does not.
        otherwise: i8,
    },
}

impl IntExpr {
    /// Construct an [`IntExpr::Cond`].
    #[must_use]
    pub fn cond(when: Condition, then: i8, otherwise: i8) -> Self {
        Self::Cond {
            when,
            then,
            otherwise,
        }
    }
}

/// Result of a skill test, as a discrete value usable in conditions.
///
/// For "succeeded by N or more" / "failed by N or more" predicates we
/// can add `SuccessBy(u8)` / `FailureBy(u8)` variants when the first
/// margin-sensitive card lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
        usage_limit: None,
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
        usage_limit: None,
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
        usage_limit: None,
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
        usage_limit: None,
    }
}

/// Construct a [`Trigger::OnEvent`] ability for the given pattern
/// and timing. Costs are empty — reactive triggers fire from the
/// engine's reaction-window plumbing, not via player activation.
#[must_use]
pub fn on_event(
    pattern: EventPattern,
    timing: EventTiming,
    kind: TriggerKind,
    effect: Effect,
) -> Ability {
    Ability {
        trigger: Trigger::OnEvent {
            pattern,
            timing,
            kind,
        },
        costs: Vec::new(),
        effect,
        usage_limit: None,
    }
}

/// Construct a mandatory **forced** [`Trigger::OnEvent`] ability
/// (`TriggerKind::Forced`). Convenience wrapper over [`on_event`].
#[must_use]
pub fn forced_on_event(pattern: EventPattern, timing: EventTiming, effect: Effect) -> Ability {
    on_event(pattern, timing, TriggerKind::Forced, effect)
}

/// Construct an optional player **reaction** [`Trigger::OnEvent`] ability
/// (`TriggerKind::Reaction`). Convenience wrapper over [`on_event`].
#[must_use]
pub fn reaction_on_event(pattern: EventPattern, timing: EventTiming, effect: Effect) -> Ability {
    on_event(pattern, timing, TriggerKind::Reaction, effect)
}

/// Construct a [`Trigger::Revelation`]-driven [`Ability`] wrapping
/// the given effect. Mirrors [`on_play`] / [`on_commit`]; costs and
/// usage limits are empty (Revelation effects pay nothing and have
/// no per-period cap — the rules treat each draw as a fresh
/// occurrence).
#[must_use]
pub fn revelation(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::Revelation,
        costs: Vec::new(),
        effect,
        usage_limit: None,
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
        usage_limit: None,
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

/// Build an [`Effect::DealDamage`] against `target` for `amount`.
#[must_use]
pub fn deal_damage(target: InvestigatorTarget, amount: u8) -> Effect {
    Effect::DealDamage { target, amount }
}

/// Build an [`Effect::DealHorror`] against `target` for `amount`.
#[must_use]
pub fn deal_horror(target: InvestigatorTarget, amount: u8) -> Effect {
    Effect::DealHorror { target, amount }
}

/// Build an [`Effect::BoostAttackDamage`] adding `amount` to the
/// in-flight Fight test's bonus damage (Vicious Blow 01025).
#[must_use]
pub fn boost_attack_damage(amount: u8) -> Effect {
    Effect::BoostAttackDamage(amount)
}

/// Build an [`Effect::DrawCards`] drawing `count` cards for `target`.
#[must_use]
pub fn draw_cards(target: InvestigatorTarget, count: u8) -> Effect {
    Effect::DrawCards { target, count }
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

/// Build an [`Effect::AdvanceCurrentAct`].
#[must_use]
pub fn advance_current_act() -> Effect {
    Effect::AdvanceCurrentAct
}

/// Build an [`Effect::Native`] referencing a host-registered Rust effect
/// by `tag` (convention: `"<cardcode>:<name>"`).
#[must_use]
pub fn native(tag: impl Into<String>) -> Effect {
    Effect::Native { tag: tag.into() }
}

/// Build an [`Effect::DiscardSelf`].
#[must_use]
pub fn discard_self() -> Effect {
    Effect::DiscardSelf
}

/// Build an [`Effect::PutIntoThreatArea`] that enters clue-less.
#[must_use]
pub fn put_into_threat_area(code: impl Into<String>) -> Effect {
    Effect::PutIntoThreatArea {
        code: code.into(),
        clues: 0,
    }
}

/// Build an [`Effect::PutIntoThreatArea`] seeding `clues` on the placed
/// instance (Cover Up 01007: "Put Cover Up into play in your threat area,
/// with 3 clues on it").
#[must_use]
pub fn put_into_threat_area_with_clues(code: impl Into<String>, clues: u8) -> Effect {
    Effect::PutIntoThreatArea {
        code: code.into(),
        clues,
    }
}

/// Build an [`Effect::Fight`] with the given combat modifier and bonus
/// damage (.38 Special: `fight(IntExpr::cond(LocationHasClues, 3, 1), 1)`).
#[must_use]
pub fn fight(combat_modifier: IntExpr, extra_damage: u8) -> Effect {
    Effect::Fight {
        combat_modifier,
        extra_damage,
    }
}

/// Build an [`Effect::Restrict`] carrying a constant [`Restriction`].
#[must_use]
pub fn restrict(restriction: Restriction) -> Effect {
    Effect::Restrict(restriction)
}

/// Build an [`Effect::SkillTest`] initiating a `skill` test against
/// `difficulty`. `on_success` runs after a passing draw, `on_fail` after a
/// failing one (with the margin in the evaluator context's `failed_by`);
/// either may be `None`. Most cards branch on exactly one side — failure
/// (the one-shot Revelation treacheries) or success (Frozen in Fear 01164).
#[must_use]
pub fn skill_test(
    skill: crate::card_data::SkillKind,
    difficulty: u8,
    on_success: Option<Effect>,
    on_fail: Option<Effect>,
) -> Effect {
    Effect::SkillTest {
        skill,
        difficulty,
        on_success: on_success.map(Box::new),
        on_fail: on_fail.map(Box::new),
    }
}

/// Build an [`Effect::ForEachPointFailed`] running `body` once per point
/// the just-resolved skill test was failed by.
#[must_use]
pub fn for_each_point_failed(body: Effect) -> Effect {
    Effect::ForEachPointFailed(Box::new(body))
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
        let ability = on_play(discover_clue(LocationTarget::YourLocation, 1));
        assert_eq!(ability.trigger, Trigger::OnPlay);
        assert!(matches!(
            ability.effect,
            Effect::DiscoverClue {
                from: LocationTarget::YourLocation,
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
            discover_clue(LocationTarget::YourLocation, 1),
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
            gain_resources(InvestigatorTarget::You, 1),
            discover_clue(LocationTarget::YourLocation, 1),
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
            discover_clue(LocationTarget::YourLocation, 1),
        );
        let with_else = if_else(
            Condition::SkillTest {
                outcome: TestOutcome::Success,
            },
            discover_clue(LocationTarget::YourLocation, 1),
            gain_resources(InvestigatorTarget::You, 1),
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
            gain_resources(InvestigatorTarget::You, 2),
            discover_clue(LocationTarget::YourLocation, 1),
        ]);
        match effect {
            Effect::ChooseOne(alts) => assert_eq!(alts.len(), 2),
            _ => panic!("expected ChooseOne"),
        }
    }

    /// `InvestigatorTarget::You` and `Active` are distinct
    /// variants — they coincide during the controller's own turn but
    /// differ during reactions across turns. The compiler enforces
    /// the difference at every match site; this test pins the
    /// distinction at the type level.
    #[test]
    fn investigator_target_controller_and_active_are_distinct() {
        assert_ne!(InvestigatorTarget::You, InvestigatorTarget::Active);
        let controller_effect = gain_resources(InvestigatorTarget::You, 1);
        let active_effect = gain_resources(InvestigatorTarget::Active, 1);
        assert_ne!(controller_effect, active_effect);
    }

    /// A deeply-nested effect tree round-trips through `serde_json`.
    /// Cheap insurance against `Box<Effect>` × nested-variant × serde
    /// derive surprises.
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
                discover_clue(LocationTarget::YourLocation, 1),
                gain_resources(InvestigatorTarget::You, 2),
            ]),
        ]);
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    #[test]
    fn native_effect_round_trips_through_serde_json() {
        let effect = native("01108:board-build");
        let json = serde_json::to_string(&effect).expect("serialize");
        let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(effect, recovered);
    }

    /// `Effect::BoostAttackDamage` (Vicious Blow 01025's "+1 damage")
    /// round-trips through serde, and the builder constructs the variant.
    #[test]
    fn boost_attack_damage_round_trips_through_serde_json() {
        let effect = boost_attack_damage(1);
        assert_eq!(effect, Effect::BoostAttackDamage(1));
        let json = serde_json::to_string(&effect).expect("serialize");
        let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(effect, recovered);
    }

    #[test]
    fn choose_surface_serde_round_trips() {
        let inv = InvestigatorTarget::chosen_anywhere();
        let loc = LocationTarget::chosen_anywhere();
        let here = InvestigatorTarget::Chosen(Choose {
            scope: EntityScope::At(LocationSet::Here),
        });
        for t in [inv, here] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(
                serde_json::from_str::<InvestigatorTarget>(&json).unwrap(),
                t
            );
        }
        let json = serde_json::to_string(&loc).unwrap();
        assert_eq!(serde_json::from_str::<LocationTarget>(&json).unwrap(), loc);
    }

    /// `Effect::DrawCards` (Guts/Perception/… "draw 1 card") round-trips
    /// through serde, and the builder constructs the variant.
    #[test]
    fn draw_cards_round_trips_through_serde_json() {
        let effect = draw_cards(InvestigatorTarget::You, 1);
        assert_eq!(
            effect,
            Effect::DrawCards {
                target: InvestigatorTarget::You,
                count: 1,
            },
        );
        let json = serde_json::to_string(&effect).expect("serialize");
        let recovered: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(effect, recovered);
    }

    /// `Effect::SkillTest` (treachery-Revelation test) with a margin-keyed
    /// `Effect::ForEachPointFailed` failure branch round-trips through serde
    /// — both new #286 variants in one tree.
    #[test]
    fn skill_test_and_for_each_point_failed_round_trip() {
        use crate::card_data::SkillKind;
        let effect = skill_test(
            SkillKind::Agility,
            3,
            None,
            Some(for_each_point_failed(deal_damage(
                InvestigatorTarget::You,
                1,
            ))),
        );
        let json = serde_json::to_string(&effect).expect("serialize");
        let back: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(effect, back);
    }

    /// Roland-Banks-shaped reaction: "after you defeat an enemy,
    /// discover 1 clue at your location" — the canonical motivating
    /// card for [`Trigger::OnEvent`]. The DSL doesn't fire it yet
    /// (engine reaction windows land in #52), but construction must
    /// work today so #55 has somewhere to land.
    #[test]
    fn on_event_builder_constructs_roland_banks_reaction() {
        let ability = on_event(
            EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            EventTiming::After,
            TriggerKind::Reaction,
            discover_clue(LocationTarget::YourLocation, 1),
        );
        assert_eq!(
            ability.trigger,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                timing: EventTiming::After,
                kind: TriggerKind::Reaction,
            },
        );
        assert!(matches!(
            ability.effect,
            Effect::DiscoverClue {
                from: LocationTarget::YourLocation,
                count: 1,
            },
        ));
        assert!(ability.costs.is_empty());
    }

    /// `OnEvent` is a distinct enum variant from existing trigger
    /// shapes — the compiler enforces the distinction at every match
    /// site, and the `by_controller` / `timing` fields differentiate
    /// the currently-expressible sub-cases. Pattern-vs-pattern
    /// distinction lands as soon as a second [`EventPattern`] variant
    /// arrives.
    #[test]
    fn on_event_distinct_from_other_triggers_and_internally() {
        let after_any = Trigger::OnEvent {
            pattern: EventPattern::EnemyDefeated {
                by_controller: false,
                code: None,
            },
            timing: EventTiming::After,
            kind: TriggerKind::Reaction,
        };
        let after_controller = Trigger::OnEvent {
            pattern: EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            timing: EventTiming::After,
            kind: TriggerKind::Reaction,
        };
        let before_controller = Trigger::OnEvent {
            pattern: EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            timing: EventTiming::Before,
            kind: TriggerKind::Reaction,
        };
        assert_ne!(after_any, Trigger::Constant);
        assert_ne!(after_any, Trigger::OnPlay);
        assert_ne!(after_any, Trigger::OnCommit);
        assert_ne!(after_any, after_controller);
        assert_ne!(after_controller, before_controller);
    }

    /// An `OnEvent`-triggered ability round-trips through `serde_json`
    /// — struct-variant × serde derive can surprise; pin the wire
    /// shape now so #52's persistence doesn't re-discover problems
    /// later. Both [`EventTiming`] variants (`After` and `Before`) are
    /// exercised independently since unit-variant × serde can fail on
    /// either alone (very rare, but the test rationale explicitly
    /// covers this surface).
    #[test]
    fn on_event_ability_round_trips_through_serde_json() {
        for timing in [EventTiming::After, EventTiming::Before] {
            let original = on_event(
                EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                timing,
                TriggerKind::Reaction,
                discover_clue(LocationTarget::YourLocation, 1),
            );
            let json = serde_json::to_string(&original).expect("serialize");
            let recovered: Ability = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(original, recovered);
        }
    }

    /// `Trigger::OnEvent` carries an explicit `TriggerKind` (forced vs
    /// reaction), and it round-trips through serde. The kind retires the
    /// old route-by-pattern dispatch (umbrella §2, Axis-B T1).
    #[test]
    fn on_event_carries_trigger_kind() {
        let t = Trigger::OnEvent {
            pattern: EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            timing: EventTiming::After,
            kind: TriggerKind::Reaction,
        };
        let json = serde_json::to_string(&t).expect("serialize");
        let back: Trigger = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(t, back);
        // Forced and Reaction are distinct.
        assert_ne!(
            t,
            Trigger::OnEvent {
                pattern: EventPattern::EnemyDefeated {
                    by_controller: true,
                    code: None,
                },
                timing: EventTiming::After,
                kind: TriggerKind::Forced,
            },
        );
    }

    /// The `revelation` builder produces the new Trigger variant with
    /// the given effect. Distinct from `OnPlay` / `OnCommit` at the type
    /// level so the compiler enforces the difference at every match site.
    #[test]
    fn revelation_builder_constructs_treachery_shape() {
        let ability = revelation(gain_resources(InvestigatorTarget::You, 1));
        assert_eq!(ability.trigger, Trigger::Revelation);
        assert!(matches!(
            ability.effect,
            Effect::GainResources {
                target: InvestigatorTarget::You,
                amount: 1,
            },
        ));
        assert!(ability.costs.is_empty());
        assert!(ability.usage_limit.is_none());
    }

    #[test]
    fn revelation_distinct_from_other_triggers() {
        assert_ne!(Trigger::Revelation, Trigger::OnPlay);
        assert_ne!(Trigger::Revelation, Trigger::OnCommit);
        assert_ne!(Trigger::Revelation, Trigger::Constant);
    }

    #[test]
    fn revelation_ability_round_trips_through_serde_json() {
        let original = revelation(gain_resources(InvestigatorTarget::You, 1));
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Ability = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    /// `EventPattern::CardRevealed { card_type: Some(...) }` and
    /// `{ card_type: None }` are distinct variants with serde
    /// round-tripping. Locks the wire shape now so #52's persistence
    /// doesn't surprise later.
    #[test]
    fn card_revealed_pattern_round_trips_through_serde_json() {
        use crate::card_data::CardType;
        let any = EventPattern::CardRevealed { card_type: None };
        let treachery = EventPattern::CardRevealed {
            card_type: Some(CardType::Treachery),
        };
        for original in [any, treachery] {
            let json = serde_json::to_string(&original).expect("serialize");
            let recovered: EventPattern = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(original, recovered);
        }
    }

    #[test]
    fn card_revealed_distinct_from_enemy_defeated() {
        use crate::card_data::CardType;
        let revealed_treachery = EventPattern::CardRevealed {
            card_type: Some(CardType::Treachery),
        };
        let enemy_defeated = EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        };
        assert_ne!(revealed_treachery, enemy_defeated);
    }

    #[test]
    fn enemy_spawned_pattern_round_trips_through_serde_json() {
        let original = EventPattern::EnemySpawned;
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    #[test]
    fn enemy_spawned_distinct_from_other_patterns() {
        let spawned = EventPattern::EnemySpawned;
        let defeated = EventPattern::EnemyDefeated {
            by_controller: true,
            code: None,
        };
        let revealed = EventPattern::CardRevealed { card_type: None };
        assert_ne!(spawned, defeated);
        assert_ne!(spawned, revealed);
    }

    #[test]
    fn entered_location_pattern_round_trips() {
        let p = EventPattern::EnteredLocation;
        let json = serde_json::to_string(&p).unwrap();
        let back: EventPattern = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn would_discover_clues_and_game_end_round_trip() {
        for p in [EventPattern::WouldDiscoverClues, EventPattern::GameEnd] {
            let json = serde_json::to_string(&p).expect("serialize");
            let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(p, back);
        }
    }

    #[test]
    fn enemy_attack_damaged_self_round_trips() {
        let p = EventPattern::EnemyAttackDamagedSelf;
        let json = serde_json::to_string(&p).expect("serialize");
        let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }

    #[test]
    fn successfully_investigated_round_trips() {
        let p = EventPattern::SuccessfullyInvestigated;
        let json = serde_json::to_string(&p).expect("serialize");
        let back: EventPattern = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }

    #[test]
    fn phase_ended_pattern_round_trips() {
        let p = EventPattern::PhaseEnded {
            phase: Phase::Enemy,
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: EventPattern = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn enemy_defeated_carries_optional_code_narrow() {
        let any = EventPattern::EnemyDefeated {
            by_controller: false,
            code: None,
        };
        let narrowed = EventPattern::EnemyDefeated {
            by_controller: false,
            code: Some("01116".into()),
        };
        assert_ne!(any, narrowed);
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
                discover_clue(LocationTarget::YourLocation, 1),
                gain_resources(InvestigatorTarget::You, 2),
            ]),
        ]);
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }
}
