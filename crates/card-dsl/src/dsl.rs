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
//! - **Stat-comparison / location-state conditions** (`AnyEnemyEngaged`,
//!   `SkillSucceededByAtLeast(N)`). Note: [`Condition::Compare`] already
//!   covers clue-count and engaged-enemy-count comparisons — these are
//!   expressible today. What remains unexpressible are conditions keyed on
//!   location state, success margin, or other quantities not yet in
//!   [`Quantity`].
//! - **Compound conditions** (`All` / `Any`) and **target-referencing
//!   conditions or quantities** (predicates about the *chosen* enemy rather
//!   than the controller). Held in [`Condition::Native`] predicates until
//!   `TODO(#609)` promotes them; its doc names the consumers.
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
//! use card_dsl::dsl::{self, ModifierScope, Stat};
//!
//! // Holy Rosary: while in play, +1 willpower.
//! let ability = dsl::constant(dsl::modify(Stat::Willpower, 1, ModifierScope::WhileInPlay));
//! ```

use serde::{Deserialize, Serialize};

use crate::card_data::{CardMetadata, CardType, SkillKind, Slot, UseKind};

// ---- triggers --------------------------------------------------

/// The **bold action designator** an ability prints (`glossary/Ability.md`,
/// "Action Designators"), verbatim:
///
/// > Some abilities have bold action designators (such as **Fight**, **Evade**,
/// > **Investigate**, or **Move**). Activating such an ability performs the
/// > designated action as described in the rules, but modified in the manner
/// > described by the ability.
///
/// It is **declared on the [`Ability`], not inferred from the effect**, because
/// the rules quote the designator and not the effect the ability happens to have.
/// `glossary/Attack_of_Opportunity.md`, verbatim:
///
/// > Each time an investigator is engaged with one or more ready enemies and
/// > takes an action other than to **fight**, to **evade**, or to activate a
/// > **parley** or **resign** ability, each of those enemies makes an attack of
/// > opportunity against the investigator...
///
/// A second consumer quotes it independently — Frozen in Fear 01164's ruling
/// (<https://arkhamdb.com/card/01164>): *"Also applies to \[action\] card
/// abilities with action designators (**Move**, **Fight**, **Evade**)."* An
/// effect-root match answers neither: Parley and Resign have no effect shape of
/// their own, and a `Seq`-wrapped Fight has the wrong one.
///
/// **The designated action costs nothing beyond the ability's own cost, and it
/// counts as that action everywhere.** Both halves are the official FAQ's
/// (`data/official-faq/Frequently_Asked_Questions.md`). On the cost: *"Paying
/// the cost of the ability is enough to initiate the action designated. There
/// is no need to spend an additional action."* — so `action_cost` on the
/// trigger is the whole price, and nothing may add a second action for the
/// designated action itself. On the counting: *"Abilities with a bold action
/// designator (like Fight, Evade or Investigate) count as an action of that
/// type"*, which is what #760 routed through the surcharge path. So an event
/// that prints one (Backstab 01051) is as much a Fight as a weapon's
/// `[action]` ability is — and the FAQ takes the same view of how the ability
/// was reached, answering for Ursula Downs 04002's *"take an investigate
/// action"* reaction that *"Ursula's reaction allows you to take any
/// investigate action, including those performed via the activate action or
/// via the play action."*
///
/// The set is the four the rules name plus [`Parley`](Self::Parley) and
/// [`Resign`](Self::Resign), which have their own glossary entries
/// (`glossary/Parley.md`, `glossary/Resign.md`) and are named by the attack-of-
/// opportunity clause above. Campaign-specific designators (`Explore`) land when
/// their campaign does.
///
/// # The designator performs; the ability modifies
///
/// Each variant carries **the modification the printed text describes**, and
/// nothing else (#805). Machete 01020's *"You get +1 \[combat\] for this
/// attack"* is [`Fight`](Self::Fight)'s `combat_modifier`; Flashlight 01087's
/// *"Your location gets -2 shroud for this investigation"* is
/// [`Investigate`](Self::Investigate)'s `shroud_modifier`. The
/// **action** is the designator's — there is no `Effect::Fight` beside it to
/// disagree with the bold word, which is what the first paragraph's rules
/// quote asks for and what a tag-plus-effect split could only achieve by
/// convention.
///
/// Two variants carry nothing, and neither is an exception to that:
///
/// - **Parley performs nothing.** `glossary/Parley.md`, in full: *"Some
///   abilities are identified with a **Parley** action designator. Such
///   abilities are initiated using the 'Activate' action."* The rules give it
///   no procedure, so the whole ability is its [`Ability::effect`] residual.
/// - **Resign performs the elimination**, per `glossary/Resign.md`: *"When an
///   investigator resigns, the investigator is eliminated by resignation (see
///   'Elimination' on page 10.) An investigator who resigns is not considered
///   to have been defeated."* Nothing in the corpus parameterises the
///   resignation, so the modification is empty.
///
/// # It lives on the ability, not on the trigger
///
/// The rules attach the designator to the ability rather than to how it was
/// reached — the FAQ answers for Ursula Downs 04002 that *"Ursula's reaction
/// allows you to take any investigate action, including those performed via
/// the activate action or via the play action."* So it is
/// [`Ability::designator`], and an *event* can print one on its
/// [`Trigger::OnPlay`] ability (Blinding Light 01066's **Evade**, Backstab
/// 01051's **Fight**) as readily as an asset prints one on its
/// [`Trigger::Activated`] ability. Those are the only two triggers that carry
/// one: it is set only by [`activated_as`] and [`on_play_as`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionDesignator {
    /// **Fight** — performs a fight action against an enemy *at the
    /// controller's location*. Every weapon in the corpus (Machete 01020,
    /// .45 Automatic 01016, Roland's .38 Special 01006, Knife 01086's two
    /// abilities).
    ///
    /// Per RR you choose an enemy at your location to attack and need not
    /// already be engaged with it, so the candidate scope is co-located, not
    /// engaged-only (#451). The engine auto-targets on exactly one candidate
    /// and suspends for a pick on two or more; the activation check rejects
    /// *zero* candidates before any cost is paid.
    ///
    /// Because the target need not be an engaged one, an `extra_damage`
    /// expression whose card text qualifies on the *attacked* enemy must read
    /// the chosen target, not the controller's engaged count — see Machete
    /// 01020's [`Condition::Native`] predicate (#592).
    Fight {
        /// Combat modifier for this attack. Evaluated at every read
        /// while the attack runs, not once at initiation, so an
        /// expression that counts something on the board answers from
        /// the board as it stands.
        combat_modifier: IntExpr,
        /// Bonus damage beyond the base 1 (.38 Special: +1). Evaluated once,
        /// at RR ST.7 when the attack's damage is determined, not at
        /// initiation: Switchblade 01044's *"If you succeed by 2 or more"*
        /// reads a margin that exists only then
        /// ([`Quantity::SkillTestSucceededBy`]), and Machete 01020's
        /// sole-engaged check reads the board the damage lands on.
        extra_damage: IntExpr,
        /// The skill the attack tests in place of `[combat]`, or `None` for
        /// combat. Shrivelling 01060: *"This attack uses \[willpower\] instead
        /// of \[combat\]"*. `combat_modifier` modifies the overriding skill,
        /// since it is a bonus *"for this attack"*, whatever the attack tests.
        skill_override: Option<SkillKind>,
        /// What happens if one of a set of chaos symbols is revealed during the
        /// attack (Shrivelling 01060's horror). See [`RevealClause`].
        reveal_clause: Option<RevealClause>,
    },
    /// **Evade** — performs an evade action against an enemy *engaged with the
    /// controller* (`glossary/Evade_Action.md`: *"an investigator can only
    /// perform an evade action against an enemy engaged with him or her"*).
    /// Blinding Light 01066, an event whose bold word rides its
    /// `Trigger::OnPlay` ability, built by [`on_play_as`].
    ///
    /// The modification is as far as Blinding Light prints it: the skill
    /// override, the reveal clause, and a success clause. Built by [`evade`]
    /// and the chaining builders [`testing`](Self::testing),
    /// [`with_reveal_clause`](Self::with_reveal_clause) and
    /// [`on_success`](Self::on_success). Two corpus printings take shapes it
    /// does not carry yet: Fire Extinguisher 02114's *"You get +3 \[agility\]
    /// for this test"* (the [`Fight`](Self::Fight) combat modifier's twin) and
    /// Strange Solution 02264's *"Evade with a base \[agility\] skill of 6"*,
    /// a **base-value replacement** (`GLOSSARY.md`, "Base value"). Each grows
    /// its field when it is built.
    Evade {
        /// The skill the evasion tests in place of `[agility]`, or `None` for
        /// agility. Blinding Light 01066: *"This evasion attempt uses
        /// \[willpower\] instead of \[agility\]."*
        skill_override: Option<SkillKind>,
        /// What happens if one of a set of chaos symbols is revealed during the
        /// evasion attempt (Blinding Light's lost action). See
        /// [`RevealClause`].
        reveal_clause: Option<RevealClause>,
        /// What happens if the evasion succeeds, after the enemy is evaded
        /// (Blinding Light's *"If you succeed, deal 1 damage to the enemy just
        /// evaded."*). It resolves inside the test, at Skill Test Timing step
        /// 7, where the evaded enemy is still the test's; the ability's
        /// residual effect runs only after the test has ended. Boxed so the
        /// designator (carried on every [`Ability`]) stays small.
        on_success: Option<Box<Effect>>,
    },
    /// **Move** — performs a move action. **No corpus card prints it**, though
    /// 29 in the snapshot do (Sled Dog 08127's *"\[action\] Exhaust X Sled
    /// Dogs: **Move.** Move X times."*, Ring Library 11624's *"\[action\]:
    /// **Move.** Move to a revealed \[\[Passageway\]\] location."*), so its
    /// modification is a *destination or a repeat count* rather than a stat
    /// row. Rejected (`TODO(#1059)`) because nothing the build compiles prints
    /// one at all, so there is no printed shape to take its payload from. Named
    /// as a designator by the rules quote above and by Frozen in Fear 01164's
    /// ruling.
    Move,
    /// **Investigate** — performs an investigate action against the
    /// controller's current location. Flashlight 01087.
    ///
    /// The mirror of [`Fight`](Self::Fight): the modifier adjusts the
    /// **location difficulty**, not the investigator's total. It is one
    /// contribution among however many the location carries (Obscuring Fog
    /// 01168's +2 is another), so the composed shroud clamps at 0 once, after
    /// all of them. The investigation reuses the base Investigate skill-test
    /// follow-up, so on success it discovers a clue like the action does,
    /// unless the card prints something that replaces the discovery
    /// ([`instead_of_discovery`](Self::Investigate::instead_of_discovery)).
    Investigate {
        /// Shroud-difficulty modifier for this investigation
        /// (Flashlight: `-2`). Evaluated at every read while the
        /// investigation runs, like [`Fight`](Self::Fight)'s combat
        /// modifier.
        shroud_modifier: IntExpr,
        /// What a success does **in place of** discovering clues, or `None`
        /// for the investigation's own discovery. Burglary 01045: *"If you
        /// succeed, instead of discovering clues, gain 3 resources."*
        ///
        /// A self-replacement the card prints on its own investigation, not a
        /// general replacement effect (#366): the test is still a successful
        /// investigation in every other respect, so "after you successfully
        /// investigate" (Dr. Milan Christopher 01033) still fires at ST.6. Only
        /// the ST.7 discovery is gone, and with it anything that raises its
        /// count, which is why Deduction 01039 does not stack with Burglary
        /// (its ruling). The effect resolves at Skill Test Timing step 7 with
        /// the test's performer as controller, as
        /// [`Evade`](Self::Evade)'s `on_success` does. Boxed so the designator
        /// (carried on every [`Ability`]) stays small.
        instead_of_discovery: Option<Box<Effect>>,
    },
    /// **Parley** — `glossary/Parley.md` in full: *"Some abilities are
    /// identified with a **Parley** action designator. Such abilities are
    /// initiated using the 'Activate' action."* The Midnight Masks cultists
    /// (01138-01140) and Mob Enforcer 01101 print one. Performs nothing; the
    /// ability's whole content is its [`Ability::effect`].
    Parley,
    /// **Resign** — the Parlor 01115 and every resign location. Eliminates the
    /// controller from the scenario **by resignation**, running
    /// `glossary/Elimination.md`'s steps 0–6 unbranched (they are one procedure
    /// for defeat and resignation alike), so the only thing distinguishing it
    /// from a defeat is the `EliminationCause::Resigned` it carries
    /// (`game-core` owns that enum; `card-dsl` has no workspace dependencies to
    /// name it through).
    ///
    /// Nullary because nothing in the corpus parameterises the resignation
    /// itself. Where a later card prints more, the extra is a **cost** or an
    /// **eligibility clause** rather than a parameter: Tear Through Time
    /// 02322's *"\[action\] Spend 2 clues: **Resign.**"* rides [`Cost`], and
    /// Escape the Tower 11617's *"\[action\] If you are at Twisting Catwalks:
    /// **Resign.**"* rides the ability's eligibility. Both hang off the
    /// [`Ability`], not off the designator.
    Resign,
    /// **Evade**, made automatically: Cunning Distraction 01078's *"**Evade.**
    /// Automatically evade all enemies at your location."* It **is** an Evade
    /// for every rule keyed on the kind (no attack of opportunity, Frozen in
    /// Fear 01164's surcharge), but no skill test is made and no engaged enemy
    /// is needed. The 01076 ruling (<https://arkhamdb.com/card/01076>): *"if an
    /// ability automatically evades 1 or more enemies, no skill test is made
    /// for the evasion attempt whatsoever. Consequentially, because no skill
    /// test is made, it is not considered a “successful” evasion."*
    ///
    /// Performs nothing, as [`Parley`](Self::Parley) does: which enemies are
    /// evaded is the ability's residual effect ([`Effect::AutoEvade`]), and
    /// the engine asks that effect whether the ability can change the game
    /// state, so the card is not playable with no enemy at your location.
    /// [`Evade`](Self::Evade) stays the test-backed action.
    AutomaticEvade,
}

impl ActionDesignator {
    /// The [`ActionClass`] this designator performs, for the rules keyed on
    /// action *kind* rather than on the basic action. `None` for the three
    /// designators no `ActionClass` variant names.
    ///
    /// Official FAQ, `Frequently_Asked_Questions.md`:
    ///
    /// > Abilities with a bold action designator (like Fight, Evade or
    /// > Investigate) **count as an action of that type**.
    ///
    /// The clause is broader than the attack-of-opportunity exemption it
    /// answers there: a designated ability *is* an action of that type for
    /// every rule that keys on one. The consumer today is
    /// [`Restriction::ExtraActionCost`], whose `actions` list is
    /// `ActionClass` — Frozen in Fear 01164 surcharges a designated Fight
    /// exactly as it surcharges a basic one (#754).
    ///
    /// The `None` arm is structural rather than a gap: `ActionClass` exists
    /// to name what `ExtraActionCost` can surcharge, and no card surcharges
    /// investigating, parleying or resigning. A new consumer wanting those
    /// grows `ActionClass` first.
    #[must_use]
    pub fn action_class(&self) -> Option<ActionClass> {
        match self {
            Self::Fight { .. } => Some(ActionClass::Fight),
            Self::Evade { .. } | Self::AutomaticEvade => Some(ActionClass::Evade),
            Self::Move => Some(ActionClass::Move),
            Self::Investigate { .. } | Self::Parley | Self::Resign => None,
        }
    }

    /// This designator's test, testing `skill` instead of the action's own
    /// (Shrivelling 01060's *"This attack uses \[willpower\] instead of
    /// \[combat\]"*, Blinding Light 01066's *"This evasion attempt uses
    /// \[willpower\] instead of \[agility\]."*).
    ///
    /// # Panics
    ///
    /// On a designator whose action has no skill test to override. A card
    /// module calling it so is an authoring mistake, caught by the first test
    /// that builds the card's abilities.
    #[must_use]
    pub fn testing(mut self, skill: SkillKind) -> Self {
        match &mut self {
            Self::Fight { skill_override, .. } | Self::Evade { skill_override, .. } => {
                *skill_override = Some(skill);
            }
            other => panic!("{other:?} carries no skill override"),
        }
        self
    }

    /// This designator's test, carrying `clause` (Shrivelling 01060's *"If a
    /// \[skull\], … symbol is revealed during this attack, take 1 horror."*,
    /// Blinding Light 01066's *"… during this evasion attempt, lose 1 action
    /// this turn."*).
    ///
    /// # Panics
    ///
    /// On a designator whose action has no skill test to reveal a token in, as
    /// [`testing`](Self::testing) does.
    #[must_use]
    pub fn with_reveal_clause(mut self, clause: RevealClause) -> Self {
        match &mut self {
            Self::Fight { reveal_clause, .. } | Self::Evade { reveal_clause, .. } => {
                *reveal_clause = Some(clause);
            }
            other => panic!("{other:?} carries no reveal clause"),
        }
        self
    }

    /// This designator's investigation, resolving `effect` on success **instead
    /// of** discovering clues (Burglary 01045's *"If you succeed, instead of
    /// discovering clues, gain 3 resources."*). See
    /// [`Investigate::instead_of_discovery`](Self::Investigate::instead_of_discovery).
    ///
    /// # Panics
    ///
    /// On a designator other than [`Investigate`](Self::Investigate), as
    /// [`testing`](Self::testing) does.
    #[must_use]
    pub fn instead_of_discovery(mut self, effect: Effect) -> Self {
        match &mut self {
            Self::Investigate {
                instead_of_discovery,
                ..
            } => *instead_of_discovery = Some(Box::new(effect)),
            other => panic!("{other:?} carries no discovery to replace"),
        }
        self
    }

    /// This designator's evasion, resolving `effect` if it succeeds (Blinding
    /// Light 01066's *"If you succeed, deal 1 damage to the enemy just
    /// evaded."*). `effect` evaluates with the test's performer as its
    /// controller, while the test is still in flight.
    ///
    /// # Panics
    ///
    /// On a designator other than [`Evade`](Self::Evade), the one whose printed
    /// success clause has a field yet, as [`testing`](Self::testing) does.
    #[must_use]
    pub fn on_success(mut self, effect: Effect) -> Self {
        match &mut self {
            Self::Evade { on_success, .. } => *on_success = Some(Box::new(effect)),
            other => panic!("{other:?} carries no success clause"),
        }
        self
    }
}

/// The symbol on a chaos token: every token but a numeric one. The card-side
/// name for `game_core::state::ChaosToken`'s symbol variants, which a card
/// names without reaching into the kernel (`card-dsl` has no workspace
/// dependencies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChaosSymbol {
    /// `[skull]`.
    Skull,
    /// `[cultist]`.
    Cultist,
    /// `[tablet]`.
    Tablet,
    /// `[elder_thing]`.
    ElderThing,
    /// `[auto_fail]`.
    AutoFail,
    /// `[elder_sign]`.
    ElderSign,
}

impl ChaosSymbol {
    /// The four scenario symbols and `[auto_fail]`: every symbol but the elder
    /// sign. Shrivelling 01060 and Blinding Light 01066 both print exactly this
    /// list (*"If a `[skull]`, `[cultist]`, `[tablet]`, `[elder_thing]`, or
    /// `[auto_fail]` symbol is revealed"*).
    pub const ALL_BUT_ELDER_SIGN: [Self; 5] = [
        Self::Skull,
        Self::Cultist,
        Self::Tablet,
        Self::ElderThing,
        Self::AutoFail,
    ];
}

/// A card's *"If a \<symbol\> is revealed during this test, \<effect\>"*,
/// carried by the skill test the card starts.
///
/// It is the initiating card's, not the token's: the chaos symbol effect a
/// token initiates is the scenario's or the investigator card's
/// ([`TokenOutcome`]), and resolves beside this one. It resolves at Skill Test
/// Timing step 3, once the token is revealed and before step 4's symbol
/// effects (Shrivelling 01060's ruling, <https://arkhamdb.com/card/01060>:
/// *"The "If a symbol is revealed..." effect triggers during Step 3 of the
/// Skill Test Timing."*). `effect` evaluates with the test's performer as its
/// controller.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevealClause {
    /// The symbols that fire it.
    pub symbols: Vec<ChaosSymbol>,
    /// What it does when one of them is revealed. Boxed, as
    /// [`Effect::SkillTest`]'s branches are, so the designator carrying it
    /// stays small.
    pub effect: Box<Effect>,
}

impl RevealClause {
    /// Whether revealing `symbol` fires this clause.
    #[must_use]
    pub fn fires_on(&self, symbol: ChaosSymbol) -> bool {
        self.symbols.contains(&symbol)
    }
}

/// When an [`Ability`] is active.
///
/// Phase-3 set. Later phases add `AtPhaseStart`/`AtPhaseEnd`,
/// `OnLeavePlay`, and additional reactive patterns as cards demand.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
// `allow`, not `expect`: the lint is pointer-width dependent and does not fire
// on wasm32, where an unfulfilled `expect` would fail `wasm-clippy`.
#[allow(
    clippy::large_enum_variant,
    reason = "the elder sign outcome was already the largest variant; the lint fired only \
              when the designator moved off Activated (#1053), which shrank the runner-up"
)]
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
    /// Fires when the owning card is drawn: from the encounter deck, or — a
    /// weakness — from its bearer's deck.
    ///
    /// Consumers are the real encounter treacheries in
    /// `cards::impls` — Ancient Evils 01166, Rotting Remains 01163,
    /// Crypt Chill 01167 and the rest — and the weaknesses that print one:
    /// Cover Up 01007, Hospital Debts 01011, The Necronomicon 01009. (The
    /// synthetic treachery that was this trigger's first consumer is gone;
    /// #878 retired the toy-scenario fixture it lived in.)
    ///
    /// Distinct from [`OnPlay`](Self::OnPlay) — Revelation fires when a card is
    /// drawn, not when it is played from hand. A treachery or enemy weakness
    /// drawn from a player deck never stays in hand: it resolves as if just
    /// drawn from the encounter deck (`glossary/Weakness.md`, #1121), and an
    /// asset weakness's Revelation resolves as it is drawn.
    ///
    /// The engine's encounter-card path (`resolve_encounter_card` in
    /// `game-core`'s `engine::dispatch`) runs every `Trigger::Revelation`
    /// ability on the drawn card through the DSL evaluator as the resolve step
    /// of its draw condition, then disposes of the card: discarding a one-shot
    /// treachery to its owner's discard pile, or spawning an enemy.
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
        /// The bold action designator, if the ability prints one, is
        /// [`Ability::designator`].
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
        /// `queue_event` dispatch it participates in — Rules Reference p.2:
        /// "all forced abilities … must resolve before any `[reaction]`
        /// abilities … may be initiated." Replaces the earlier
        /// route-by-`EventPattern` heuristic (which forced twin patterns
        /// for one game moment); the unified `SkillTestResolved` pattern,
        /// shared by forced and reaction listeners, now relies on this
        /// `kind` to route to the right phase.
        kind: TriggerKind,
    },
    /// Fires when the investigator's **elder-sign** chaos token (`[O]`)
    /// is revealed during a skill test they are taking. The elder-sign is
    /// the investigator's *own* symbol token: its effect is sourced from
    /// the investigator card rather than the scenario bag.
    ///
    /// `outcome` is the sign's [`TokenOutcome`], the same shape a scenario's
    /// symbol table returns for `[skull]` / `[cultist]` / `[tablet]` /
    /// `[elder_thing]`. Its `modifier` is an [`IntExpr`] so board-state-dependent
    /// values (Roland Banks's "+1 for each clue on your location" →
    /// `Count(CluesAtControllerLocation)`) stay live: ST.3 copies the
    /// expression **unevaluated** into a test-scoped recorded row, and the
    /// ST.5 fold evaluates it at every read (ADR 0005, ADR 0007). Nothing
    /// freezes it at reveal.
    ///
    /// Config-on-trigger, like [`Activated`](Self::Activated) /
    /// [`OnSkillTestResolution`](Self::OnSkillTestResolution). The ability's
    /// own `effect` slot stays the inert empty `Seq`; the sign's effects live
    /// in the outcome's `immediate` / `on_success` / `on_fail` arms.
    ///
    /// The arms run with the investigator card as their source, so anything
    /// they latch names that card. `immediate` runs at ST.4, where the
    /// ability initiates; `on_success` / `on_fail` ("If you succeed, gain 2
    /// resources", "Skids" O'Toole 01003) are skill test results and run at
    /// ST.7.
    ///
    /// An automatic success is an `immediate` effect, not an override of the
    /// outcome: Wendy Adams 01005's sign is
    /// `If(<Amulet in play>, AutoResolve(AutomaticSuccess))`, a test-scoped
    /// determination that ST.6 reads (ADR 0007). The token has already been
    /// revealed, so nothing is skipped
    /// (`data/rules-reference/rules/glossary/Automatic_Failure_Success.md`):
    ///
    /// > If a chaos token effect causes an investigator to automatically
    /// > succeed or fail at a skill test, continue with Steps 3 and 4, as
    /// > normal.
    ///
    /// The determination substitutes the difficulty, not the skill value, so
    /// ST.5 still determines the modified skill value. Wendy's ruling
    /// (<https://arkhamdb.com/card/01005>):
    ///
    /// > If a skill test automatically succeeds, the total difficulty of that
    /// > test is considered 0.
    ElderSign {
        /// What the elder sign does when revealed.
        outcome: TokenOutcome,
    },
}

/// What a revealed chaos-symbol token does: one **chaos symbol effect**
/// (`GLOSSARY.md`), whether it comes from the scenario reference card
/// (`[skull]` / `[cultist]` / `[tablet]` / `[elder_thing]`) or from the
/// performing investigator's elder-sign ability. ST.4
/// (`Appendix_II_Timing_and_Gameplay.md`):
///
/// > Each of the following symbols indicates that an ability on the scenario
/// > reference card must initiate: \[skull\], \[cultist\], \[tablet\], or
/// > \[elder_thing\].
/// >
/// > The \[elder_sign\] symbol indicates that the \[elder_sign\] ability on the
/// > investigator card belonging to the player performing the test must
/// > initiate.
///
/// The four parts land at different steps of the test:
///
/// - `modifier` is copied unevaluated into a test-scoped recorded row at ST.3
///   and read live by the ST.5 fold.
/// - `immediate` runs at ST.4, before the modified skill value is determined.
/// - `on_success` / `on_fail` run at ST.7, after the test's own result,
///   according to the success or failure established at ST.6.
///
/// Built by literal (scenario symbol tables do so), so it is deliberately not
/// `#[non_exhaustive]`. [`Default`] is the inert outcome: `+0` and no effects.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenOutcome {
    /// The token's contribution to the investigator's skill value.
    pub modifier: IntExpr,
    /// The unconditional part, resolved at ST.4.
    pub immediate: Option<Effect>,
    /// The part that resolves at ST.7 only if the test succeeded.
    pub on_success: Option<Effect>,
    /// The part that resolves at ST.7 only if the test failed.
    pub on_fail: Option<Effect>,
}

impl Default for TokenOutcome {
    fn default() -> Self {
        Self::modifier(IntExpr::Lit(0))
    }
}

impl TokenOutcome {
    /// A modifier-only outcome: `modifier`, and no effects.
    #[must_use]
    pub fn modifier(modifier: IntExpr) -> Self {
        Self {
            modifier,
            immediate: None,
            on_success: None,
            on_fail: None,
        }
    }
}

/// Whether an [`Trigger::OnEvent`] ability resolves mandatorily (forced)
/// or is an optional player reaction.
///
/// Forced abilities all resolve before any reaction abilities at the same
/// timing point (Rules Reference p.2), and the engine's `queue_event`
/// dispatch keys its two phases off this distinction rather than guessing
/// from the [`EventPattern`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggerKind {
    /// Mandatory; resolves automatically (the player only orders
    /// simultaneous ones). Phase 1 of `queue_event`.
    Forced,
    /// Optional; the controller may use it in the reaction window.
    /// Phase 2 of `queue_event`.
    Reaction,
}

/// Which engine event(s) an [`Trigger::OnEvent`] ability listens for.
///
/// Grows as cards demand new patterns. Every pattern listens to exactly one
/// [`TriggeringCondition`], answered by the exhaustive
/// [`condition`](Self::condition), so a new variant does not compile until it
/// says which condition it belongs to. A pattern with no condition that could
/// match it is decoration, and is not added until a card needs it together
/// with its condition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventPattern {
    /// An enemy was defeated. `by_controller` narrows the match to
    /// defeats credited to the ability's controller — Roland Banks's
    /// "after **you** defeat an enemy" sets this `true`; an
    /// unqualified "after an enemy is defeated" would set it `false`.
    EnemyDefeated {
        /// If `true`, only fires when the controller of this ability
        /// is credited with the defeat (the `by` field of
        /// `game_core::event::Event::EnemyDefeated`). If `false`, any defeat
        /// matches.
        by_controller: bool,
        /// Narrow the match to a specific defeated enemy printed code
        /// (e.g. the Ghoul Priest's `"01116"` for Act 3's objective).
        /// `None` matches any enemy's defeat (e.g. Roland's reaction).
        code: Option<String>,
    },
    /// An investigator entered the location this ability is printed on
    /// (Forced "after you enter \<location\>" effects: Attic `01113`
    /// takes 1 horror, Cellar `01114` takes 1 damage).
    ///
    /// Intentionally bare: *this location* is part of the pattern's
    /// definition, so the engine's matcher hears it only on the entered
    /// location's own card, and binds *you* = the entering investigator — no
    /// narrowing fields needed.
    EnteredLocation,
    /// A game phase began — `Appendix_II_Timing_and_Gameplay.md` steps 1.1,
    /// 2.1, 3.1 and 4.1, each of which is *"an important game milestone that
    /// may be referenced in card text, either as a point at which an ability
    /// may or must resolve, or as a point at which a delayed effect resolves or
    /// a lasting effect expires."*
    ///
    /// The mirror of [`PhaseEnded`](Self::PhaseEnded), and wired at all four
    /// starts (#697). Dunwich supplies the first consumers — Hunting Horror
    /// 02141 and Peter Clover 02079 print *"**Forced** - At the start of the
    /// enemy phase: …"*, as does agenda 02065.
    PhaseStarted { phase: Phase },
    /// A game phase ended. Forced agenda/act effects keyed to a phase
    /// boundary listen here: agenda `01107` moves Ghouls at
    /// `PhaseEnded { phase: Enemy }`. (Its end-of-round doom keys off
    /// [`RoundEnded`](Self::RoundEnded), not `PhaseEnded { Upkeep }`.)
    ///
    /// Wired at all four phase-ends (#697); `Appendix_II_Timing_and_Gameplay.md`
    /// step 1.5 gives the end of a phase the same milestone sentence its step
    /// 1.1 gives the beginning.
    PhaseEnded { phase: Phase },
    /// The act this ability is printed on advanced (its reverse side
    /// resolves). *This act* is part of the definition: the engine's matcher
    /// hears it only on the advancing act, and binds controller = the lead
    /// investigator (board-wide reverse effects ignore it).
    ActAdvanced,
    /// The agenda this ability is printed on advanced (its reverse side
    /// resolves on doom) — the mirror of [`ActAdvanced`](Self::ActAdvanced),
    /// heard only on the advancing agenda and bound to the lead
    /// investigator. The Gathering's agenda reverses
    /// listen here: 01105 (lead's discard/horror choice) and 01106
    /// (dig the encounter deck until a `Ghoul` enemy, lead draws it).
    AgendaAdvanced,
    /// The round ended (Rules Reference p.24: the round ends at the close
    /// of the upkeep phase). Forced agenda/act effects keyed to "at the
    /// end of the round" listen here — agenda `01107` places doom, Dissonant
    /// Voices 01165 discards itself. Board-wide: heard on every card in play.
    /// Distinct
    /// from `PhaseEnded { Upkeep }` so an "end of upkeep phase" and an
    /// "end of round" card can coexist.
    RoundEnded,
    /// The investigator's turn ended (Rules Reference p.24 step 2.2.2,
    /// "Forced – At the end of your turn"). *Your* is part of the
    /// definition: the engine's matcher hears it only when the ability's
    /// controller is the investigator whose turn ended. First consumer:
    /// Frozen in Fear (01164), C4c (#235).
    EndOfTurn,
    /// A skill test resolved with the given `outcome` (RR ST.6). The
    /// card-facing narrowing of the engine's ST.6→ST.7 timing point — the
    /// general form of which "after you successfully investigate" (Dr. Milan
    /// 01033 reaction, Obscuring Fog 01168 forced) is the `{ Success,
    /// Investigate }` case. `kind: None` matches any test type; `Some(k)`
    /// narrows to that type. Forced vs reaction is the `OnEvent { kind }`
    /// distinction (Obscuring Fog `Forced`, Dr. Milan `Reaction`), not a
    /// pattern distinction — both share this pattern. A forced ability on a
    /// location attachment binds *you* = the testing investigator, and
    /// [`tested_location`](Self::SkillTestResolved::tested_location) says
    /// whether it hears only its own location's tests. (Slice D #423;
    /// collapses the #212/#213 forced/reaction pattern split for this timing
    /// point.)
    SkillTestResolved {
        /// Whether the listener fires on a passed or failed test.
        outcome: TestOutcome,
        /// Narrow to a test type, or `None` for any.
        kind: Option<SkillTestKind>,
        /// If `true`, only fires on a test **the controller of this ability is
        /// performing** — the *"after **you** …"* the corpus overwhelmingly
        /// prints (Dr. Milan 01033, Obscuring Fog 01168). If `false` the
        /// pattern is unqualified: any investigator's test matches, and the
        /// card narrows it itself.
        ///
        /// Mirrors [`EnemyDefeated`](Self::EnemyDefeated)'s field of the same
        /// name, and exists because the narrowing cannot be pushed anywhere
        /// else: the reaction matcher hard-requires *test investigator ==
        /// controller* **before** a candidate is minted, so an eligibility
        /// predicate — which runs afterwards — can never widen it back out.
        ///
        /// Two consumers clear `standards.md`'s threshold, both reacting to
        /// somebody else's test: Lita Chantler 01117 (*"\[reaction\] When an
        /// investigator at your location successfully attacks a
        /// \[\[Monster\]\] enemy: That investigator deals +1 damage."*) and
        /// Try and Try Again 02309 (*"\[reaction\] After a skill test is
        /// failed, if a skill card you own is committed to that test…"*).
        ///
        /// **A bool, so the one thing it cannot say is *"another
        /// investigator's test, excluding mine"*.** No corpus card as a
        /// resolution trigger wants that: the variation the corpus does print
        /// is entirely in **location scope** (Analytical Mind 03010's *"at
        /// another location"*, Bestow Resolve 09032's *"or a connecting
        /// location"*). The one location scope that is part of a declaration
        /// is [`tested_location`](Self::SkillTestResolved::tested_location);
        /// the rest stays the card's own, in a native eligibility condition.
        by_controller: bool,
        /// Which tested location the ability listens to — Obscuring Fog
        /// 01168's *"After **attached location** is successfully
        /// investigated"* is [`TestedLocationScope::Attached`]; every other
        /// corpus consumer is [`TestedLocationScope::Any`].
        ///
        /// A field rather than a matcher rule because the pattern has both
        /// scoped and unscoped consumers: the trigger scan walks the whole
        /// board (ADR 0018), so the word *"attached"* has to be on the
        /// declaration for the scan to read it.
        tested_location: TestedLocationScope,
        /// How far the test passed or failed by, or `None` for any margin —
        /// Scavenging 01073's *"After you successfully investigate **by 2 or
        /// more**"* and "Look what I found!" 01079's *"after you fail a skill
        /// test **by 2 or less**"*. See [`MarginScope`] for what the amount is
        /// measured against.
        ///
        /// Trigger scope rather than an eligibility predicate, which is how
        /// Lita Chantler 01117 narrows her own condition (#1006): with two
        /// consumers the narrowing belongs on the declaration.
        margin: Option<MarginScope>,
    },
    /// An investigator discovers one or more clues — the **one** triggering
    /// condition for clue discovery, in all three cells (#703). Which cell an
    /// ability lands in is its
    /// [`EventTiming`], not a second pattern:
    /// [`When`](EventTiming::When) interrupts the discovery (a replacement
    /// effect, before the clues move), [`At`](EventTiming::At) and
    /// [`After`](EventTiming::After) resolve once they have.
    ///
    /// The condition is coordinator-owned, so its `when` cell is walked: the
    /// discovery itself happens between the `when` and `at` cells. First
    /// consumer: Cover Up 01007's "`[reaction]` When you would discover 1 or
    /// more clues at your location: Discard that many clues from Cover Up
    /// instead." — `When` + [`Effect::Cancel`], which prevents the discovery
    /// from resolving at all (`glossary/Instead.md`).
    ///
    /// *You* and *at your location* are part of the definition: the engine's
    /// matcher hears it only when the ability's controller is the discoverer,
    /// standing at the discovery location. It binds, for a `when` ability, the
    /// would-be discovery's count (Cover Up's "that many"). That
    /// count is what would **actually** be discovered — capped at the clues
    /// present, per the **Discovery** entry in `GLOSSARY.md` (#471).
    DiscoverClues,
    /// The game ended (a scenario resolution latched) — or, at Elimination step
    /// 0, ended for one eliminated investigator's weaknesses only. Heard on
    /// every card in play. First consumer: Cover Up
    /// 01007's "Forced - When the game ends, if there are any clues on
    /// Cover Up: You suffer 1 mental trauma." (C5a #236.)
    GameEnd,
    /// An enemy attack assigned damage to the asset this ability is printed on
    /// (the soaked ally). Bare — the engine binds *self* = an asset the
    /// assignment gives damage to, the way
    /// [`EnteredLocation`](Self::EnteredLocation) / [`EndOfTurn`](Self::EndOfTurn)
    /// bind theirs, and binds the attacking enemy into the `EvalContext`.
    ///
    /// Pairs with the engine's `DamageDealt` condition — announced once Rules
    /// Reference step 1's assignment exists, before anything is on any card,
    /// with the placement as its resolve step — narrowed by this
    /// pattern to an enemy *attack* (non-attack `Effect::Deal` harm does not
    /// match). The `when` cell is therefore the window the rules name *between*
    /// assigning and placing, which is what its one consumer prints: Guard Dog
    /// 01021's "\[reaction\] **When** an enemy attack deals damage to Guard Dog:
    /// Deal 1 damage to the attacking enemy." So the retaliate resolves *before*
    /// the damage lands on Guard Dog, and a Guard Dog killed by the attack still
    /// bites back (`data/arkhamdb-faq/core/01021.md`). See
    /// `docs/adr/0009-damage-is-assigned-then-placed.md`. (C5b #237.)
    ///
    /// **The name deliberately no longer mirrors its condition.** Mirroring it
    /// would mean a general "damage was assigned to self" pattern carrying the
    /// damage source, and that is a DSL primitive no second card wants yet — the
    /// threshold `docs/agents/standards.md` sets for adding one. This name says
    /// what the *card* declares, which is the narrower thing: an enemy
    /// **attack** dealing damage to self. The generalization waits for the card
    /// that contests it.
    EnemyAttackDamagedSelf,
    /// At least 1 of `kind` was **placed** on the card `scope` names — step 2
    /// of dealing damage, once the assignment has settled (ADR 0009). Agnes
    /// Baker 01004: *"`[reaction]` After 1 or more horror is placed on Agnes
    /// Baker: Deal 1 damage to an enemy at your location. (Limit once per
    /// phase.)"* is `{ Horror, YourInvestigator }` in the `after` cell.
    ///
    /// **Placed on, not taken by** (`GLOSSARY.md`, "Taken by you / placed on
    /// your investigator"): the scope reads only the share that lands on the
    /// named card, so horror the controller assigns to a soaking asset does not
    /// count. Her ruling (<https://arkhamdb.com/card/01004>): *"You cannot use
    /// Agnes' ability if you assign horror to an asset."* The "taken by you"
    /// scope, which counts assets too (`glossary/You_Your.md`), is #781's.
    HarmPlaced {
        /// Which harm must have been placed, at least 1 of it.
        kind: HarmKind,
        /// Which card it must have been placed on.
        scope: HarmPlacedScope,
    },
    /// An enemy attacks an investigator (RR p.25 step 3.3) — one triggering
    /// condition in all three cells since #704, so a card declares this pattern
    /// plus the trigger word it prints:
    ///
    /// - [`When`](EventTiming::When) — the cancel/replacement window. Dodge
    ///   01023: *"Fast. Play when an enemy attacks an investigator at your
    ///   location. / Cancel that attack."*
    /// - [`At`](EventTiming::At) — between the two. No corpus card yet.
    /// - [`After`](EventTiming::After) — once the damage and horror have landed.
    ///   Silver Twilight Acolyte 01102: *"**Forced** - After Silver Twilight
    ///   Acolyte attacks: Place 1 doom on the current agenda."*
    ///
    /// Two independent scopes, because the corpus varies both: which enemy is
    /// attacking (*"Silver Twilight Acolyte attacks"* is
    /// [`AttackerScope::This`]) and which investigator is attacked (*"an
    /// investigator at your location"* is [`TargetScope::AtYourLocation`]).
    /// Dodge is `{ Any, AtYourLocation }`; the Acolyte is `{ This, Any }`. The
    /// trigger scan walks the whole board (ADR 0018), so neither scope can be
    /// left to where the scan looks.
    ///
    /// A cancel in the `when` cell suppresses the rest of the sequence, so an
    /// `at`/`after` ability on a dodged attack does not fire (#714;
    /// `data/arkhamdb-faq/core/01023.md`).
    EnemyAttacks {
        /// Which attacking enemy the ability listens to.
        attacker: AttackerScope,
        /// Which attacked investigator the ability listens to.
        target: TargetScope,
    },
    /// The card this ability is printed on entered play (Research Librarian
    /// 01032: "`[reaction]` After Research Librarian enters play: …"). Bare and
    /// **self-referential** — the engine's matcher hears it only on the
    /// just-entered instance, binding *you* = the controller. A general "after
    /// any card enters play" reaction is out of scope.
    EnteredPlay,
    /// An investigator left the location this ability's source is attached to
    /// (Barricade 01038's "Forced — When an investigator leaves attached
    /// location"). Bare: *attached location* is part of the definition, so the
    /// engine's matcher hears it only on an attachment of the *left* location,
    /// and binds the leaving investigator.
    LeftLocation,
    /// **You** played a card the `filter` passes — Heirloom of Hyperborea
    /// 01012's *"\[reaction\] After you play a \[\[Spell\]\] card: Draw 1
    /// card."* Scoped to plays by the ability's controller; the filter reads
    /// the played card's printed metadata ([`CardFilter::matches`]).
    ///
    /// The condition is the play's *completion*, after Appendix I step 4: the
    /// 01012 ruling puts the trigger *"after the Spell card has resolved in
    /// full"* — an event already in its owner's discard pile, an asset already
    /// in play.
    CardPlayed {
        /// Which played cards the ability listens to.
        filter: CardFilter,
    },
    /// **You** drew an encounter card the `filter` passes — Ward of Protection
    /// 01065's *"Fast. Play when you draw a non-weakness treachery card."*
    /// Scoped to draws by the ability's controller; the filter reads the drawn
    /// card's printed metadata ([`CardFilter::matches`]).
    ///
    /// The condition's own resolution is the card's revelation, so a `when`
    /// ability that cancels it (Ward's *"Cancel that card's revelation
    /// effect"*) prevents the revelation, and the card is still disposed of.
    EncounterCardDrawn {
        /// Which drawn cards the ability listens to.
        filter: CardFilter,
    },
    /// **You** evaded an enemy — Pickpocketing 01046's *"\[reaction\] After
    /// you evade an enemy, exhaust Pickpocketing: Draw 1 card."* Scoped to
    /// evasions by the ability's controller.
    ///
    /// One evasion, however it came about: the Evade action, a designated
    /// **Evade** (Blinding Light 01066), and an automatic evade (#780) all
    /// announce it, which Pickpocketing's ruling needs
    /// (<https://arkhamdb.com/card/01046>): *"the auto-evade effect from
    /// Breaking and Entering can trigger the reaction on Pickpocketing (0)"*.
    /// The condition's own resolution is the disengage and the exhaust, so an
    /// `after` ability sees the enemy already evaded.
    ///
    /// One hand-written consumer in this slice, below `standards.md`'s
    /// threshold for a new pattern; a trigger has no card-local alternative,
    /// and Earl Sawyer 02218 (Dunwich) prints the same condition: *"\[reaction\]
    /// After you evade an enemy, exhaust Earl Sawyer: Draw 1 card."*
    EnemyEvaded,
    /// **Your** turn began (Appendix II step 2.2, *"Next investigator's turn
    /// begins."*). Scoped to the ability's controller: the engine's matcher
    /// hears it only for the investigator whose turn it is. Its `after` cell is
    /// the first opening of the post-2.2 player window, not the re-openings
    /// step 2.2.1 returns to after each action.
    ///
    /// On the Lam 01010's *"Fast. Play after your turn begins."* is the timing
    /// point read as a triggering condition — `glossary/Fast.md`: *"If the
    /// instructions specify when/after a timing point, the card may be played
    /// as if the described timing point were a triggering condition for playing
    /// the card."* — so it is played from hand in this cell, as Dodge 01023 is
    /// in an attack's.
    ///
    /// One Core consumer, below `standards.md`'s threshold for a new pattern; a
    /// play timing has no card-local alternative, and three cards outside the
    /// Core print the same condition: Leo Anderson 04001's *"\[reaction\] After
    /// your turn begins: Play an \[\[Ally\]\] asset, reducing its cost by 1."*,
    /// Tristan Botley 07194's *"\[reaction\] After your turn begins: Choose two
    /// skills."* and Jenny Barnes 90084's *"\[reaction\] After your turn begins:
    /// Play a \[\[Talent\]\] asset, reducing its cost by 1."*
    TurnBegan,
    /// **You** revealed a chaos token — Wendy Adams 01005's *"\[reaction\]
    /// When you reveal a chaos token, choose and discard 1 card from your hand:
    /// Cancel that chaos token and return it to the bag. Reveal a new chaos
    /// token."* Scoped to the ability's controller: the engine's matcher hears
    /// it only for the investigator performing the test.
    ///
    /// The condition's own resolution is everything Skill Test Timing step 3
    /// does with the token once it is out — its modifier row (or its automatic
    /// failure), the frame's revealed token, step 4's chaos symbol effect and
    /// the initiating card's reveal clause — so a `when` ability that cancels
    /// it leaves the test as if the token had never been revealed, Wendy's
    /// ruling (<https://arkhamdb.com/card/01005>): *"treat it as if you've
    /// never revealed that token, for purposes of effects that trigger if a
    /// specific symbol is revealed"*. A test whose result is known before step
    /// 3 reveals no token, so it never announces this.
    ///
    /// Two consumers: Wendy in its `when` cell, and Ritual Candles 02029
    /// (Dunwich) in its `after` cell, *"\[reaction\] After a `[skull]`,
    /// `[cultist]`, `[tablet]`, or `[elder_thing]` symbol is revealed during
    /// a test you are performing: You get +1 skill value for this test."* The
    /// symbol narrowing arrives with the Candles. Wendy is its only corpus
    /// consumer, so it is a spec-approved exception to `standards.md`'s
    /// DSL-primitive threshold (#780).
    ChaosTokenRevealed,
}

impl EventPattern {
    /// The one triggering condition this pattern listens to. An **exhaustive**
    /// match: a new pattern cannot compile without naming its condition, and
    /// the engine's matcher refuses a pattern whose condition differs from the
    /// timing event's before it reads any of the pattern's narrowing.
    ///
    /// The engine's side of the pairing is `TimingEvent::condition`, also
    /// exhaustive, so neither a new event nor a new pattern can be silently
    /// left unpaired: the classification discipline of
    /// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`,
    /// extended from who resolves a condition to which patterns it matches.
    pub fn condition(&self) -> TriggeringCondition {
        match self {
            EventPattern::EnemyDefeated { .. } => TriggeringCondition::EnemyDefeated,
            EventPattern::EnteredLocation => TriggeringCondition::EnteredLocation,
            EventPattern::PhaseStarted { .. } => TriggeringCondition::PhaseStarted,
            EventPattern::PhaseEnded { .. } => TriggeringCondition::PhaseEnded,
            EventPattern::ActAdvanced => TriggeringCondition::ActAdvanced,
            EventPattern::AgendaAdvanced => TriggeringCondition::AgendaAdvanced,
            EventPattern::RoundEnded => TriggeringCondition::RoundEnded,
            EventPattern::EndOfTurn => TriggeringCondition::EndOfTurn,
            EventPattern::SkillTestResolved { .. } => TriggeringCondition::SkillTestResolved,
            EventPattern::DiscoverClues => TriggeringCondition::DiscoverClues,
            EventPattern::GameEnd => TriggeringCondition::GameEnd,
            // "When an enemy attack deals damage to Guard Dog" (01021) is the
            // card's narrowing of the damage *assignment*, not a condition of
            // its own (ADR 0009).
            EventPattern::EnemyAttackDamagedSelf => TriggeringCondition::DamageDealt,
            EventPattern::HarmPlaced { .. } => TriggeringCondition::DamagePlaced,
            EventPattern::EnemyAttacks { .. } => TriggeringCondition::EnemyAttacks,
            EventPattern::EnteredPlay => TriggeringCondition::EnteredPlay,
            EventPattern::LeftLocation => TriggeringCondition::LeftLocation,
            EventPattern::CardPlayed { .. } => TriggeringCondition::CardPlayed,
            EventPattern::EncounterCardDrawn { .. } => TriggeringCondition::EncounterCardDrawn,
            EventPattern::EnemyEvaded => TriggeringCondition::EnemyEvaded,
            EventPattern::TurnBegan => TriggeringCondition::TurnBegan,
            EventPattern::ChaosTokenRevealed => TriggeringCondition::ChaosTokenRevealed,
        }
    }

    /// The filter a pattern narrows its condition's card by — the played card
    /// of [`EventPattern::CardPlayed`], the drawn card of
    /// [`EventPattern::EncounterCardDrawn`] — or `None` for a pattern whose
    /// condition names no card to filter. Exhaustive, like [`Self::condition`],
    /// so a new pattern carrying a filter cannot forget to say so.
    #[must_use]
    pub fn card_filter(&self) -> Option<&CardFilter> {
        match self {
            EventPattern::CardPlayed { filter } | EventPattern::EncounterCardDrawn { filter } => {
                Some(filter)
            }
            EventPattern::EnemyDefeated { .. }
            | EventPattern::EnteredLocation
            | EventPattern::PhaseStarted { .. }
            | EventPattern::PhaseEnded { .. }
            | EventPattern::ActAdvanced
            | EventPattern::AgendaAdvanced
            | EventPattern::RoundEnded
            | EventPattern::EndOfTurn
            | EventPattern::SkillTestResolved { .. }
            | EventPattern::DiscoverClues
            | EventPattern::GameEnd
            | EventPattern::EnemyAttackDamagedSelf
            | EventPattern::HarmPlaced { .. }
            | EventPattern::EnemyAttacks { .. }
            | EventPattern::EnteredPlay
            | EventPattern::LeftLocation
            | EventPattern::EnemyEvaded
            | EventPattern::TurnBegan
            | EventPattern::ChaosTokenRevealed => None,
        }
    }
}

/// Which card an [`EventPattern::HarmPlaced`] ability reads the placement on.
/// Only the scope a corpus card prints; *"a card you control"* (Mark Harrigan
/// 03001) arrives with the first card that needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HarmPlacedScope {
    /// The controller's own investigator card — Agnes Baker 01004's *"placed
    /// on Agnes Baker"*, printed on her investigator card. Assets the
    /// controller soaks with are not part of it.
    YourInvestigator,
}

/// Which attacking enemy an [`EventPattern::EnemyAttacks`] ability listens
/// to. Only the scopes a corpus card prints; *"a \[\[Trait\]\] enemy"* arrives with
/// the first card that needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttackerScope {
    /// The enemy the ability is printed on — Silver Twilight Acolyte 01102's
    /// *"After Silver Twilight Acolyte attacks"*.
    This,
    /// Any enemy — Dodge 01023's *"when an enemy attacks"*.
    Any,
}

/// Which attacked investigator an [`EventPattern::EnemyAttacks`] ability
/// listens to. Only the scopes a corpus card prints; *"attacks you"* arrives
/// with the first card that needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetScope {
    /// An investigator at the controller's location — Dodge 01023's *"an
    /// investigator at your location"*. An investigator at no location is at
    /// nobody's location, so never matches.
    AtYourLocation,
    /// Any investigator — Silver Twilight Acolyte 01102's unqualified
    /// *"attacks"*.
    Any,
}

/// Which tested location an [`EventPattern::SkillTestResolved`] ability
/// listens to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TestedLocationScope {
    /// Any test, at any location or none.
    Any,
    /// A test whose tested location is the one the ability's card is attached
    /// to — Obscuring Fog 01168's *"After attached location is successfully
    /// investigated"*.
    Attached,
}

/// The margin an [`EventPattern::SkillTestResolved`] ability listens to: the
/// test's margin compared under `op` against `amount`.
///
/// The margin is measured **in the direction of the outcome**, as the printed
/// text reads it: on a success it is how far the test succeeded by, on a
/// failure how far it failed by, never negative either way. So *"by 2 or
/// more"* on a success is `{ op: Ge, amount: 2 }` and *"by 2 or less"* on a
/// failure is `{ op: Le, amount: 2 }`.
///
/// It is read from the result the test stores when its outcome is
/// determined, at the moment the scan runs, so a later re-total that moves
/// only the margin is seen by the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarginScope {
    /// How the margin compares against `amount`.
    pub op: CmpOp,
    /// The printed amount.
    pub amount: u8,
}

impl MarginScope {
    /// *"by `amount` or more"*.
    #[must_use]
    pub const fn at_least(amount: u8) -> Self {
        Self {
            op: CmpOp::Ge,
            amount,
        }
    }

    /// *"by `amount` or less"*.
    #[must_use]
    pub const fn at_most(amount: u8) -> Self {
        Self {
            op: CmpOp::Le,
            amount,
        }
    }

    /// Whether a margin of `margin` (measured in the outcome's direction)
    /// satisfies this scope.
    #[must_use]
    pub fn admits(self, margin: u8) -> bool {
        match self.op {
            CmpOp::Eq => margin == self.amount,
            CmpOp::Ne => margin != self.amount,
            CmpOp::Lt => margin < self.amount,
            CmpOp::Le => margin <= self.amount,
            CmpOp::Gt => margin > self.amount,
            CmpOp::Ge => margin >= self.amount,
        }
    }
}

/// A **triggering condition**: the one game occurrence an [`EventPattern`]
/// listens to and a timing event announces. `glossary/Nested_Sequences.md`:
/// *"Each time a triggering condition occurs, the following sequence is
/// followed: …"*. Every condition has all three timing cells, so a condition
/// is not a cell.
///
/// The meeting point of the two exhaustive maps, [`EventPattern::condition`]
/// here and `TimingEvent::condition` in `game-core`, which is why it lives in
/// `card-dsl`, below both. Fieldless: what narrows a match within one condition
/// (a phase, a test outcome, *"you"*) is on the pattern and the event, not here.
///
/// A condition can have no pattern yet: the engine announces what the rules
/// define whether or not a corpus card listens to it. (`DamagePlaced` was one
/// until Agnes Baker 01004's [`EventPattern::HarmPlaced`].)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TriggeringCondition {
    /// An investigator entered a location.
    EnteredLocation,
    /// A game phase began.
    PhaseStarted,
    /// A game phase ended.
    PhaseEnded,
    /// An act advanced.
    ActAdvanced,
    /// An agenda advanced.
    AgendaAdvanced,
    /// An enemy was defeated.
    EnemyDefeated,
    /// The round ended.
    RoundEnded,
    /// An investigator's turn ended.
    EndOfTurn,
    /// The game ended, for everyone or for one eliminated investigator's
    /// weaknesses (a weakness prints one *"when the game ends"*).
    GameEnd,
    /// Damage and/or horror was dealt: announced once it is assigned (step 1
    /// of dealing it), with the placement as its resolve step (ADR 0009).
    DamageDealt,
    /// Assigned damage and/or horror was placed (step 2), nested inside
    /// [`DamageDealt`](Self::DamageDealt)'s resolve step (ADR 0009).
    DamagePlaced,
    /// A skill test's success or failure was determined (RR ST.6).
    SkillTestResolved,
    /// An enemy attacks an investigator.
    EnemyAttacks,
    /// An investigator discovers clues.
    DiscoverClues,
    /// A card entered play.
    EnteredPlay,
    /// An investigator left a location.
    LeftLocation,
    /// An investigator's play of a card completed (after Appendix I step 4).
    CardPlayed,
    /// An investigator drew an encounter card (framework step 1.4, or a card
    /// effect's draw); its revelation is the condition's resolution.
    EncounterCardDrawn,
    /// An investigator evaded an enemy; the disengage and the exhaust are the
    /// condition's resolution.
    EnemyEvaded,
    /// An investigator's turn began (Appendix II step 2.2).
    TurnBegan,
    /// An investigator performing a skill test revealed a chaos token (Skill
    /// Test Timing step 3); what the token does to the test is the
    /// condition's resolution.
    ChaosTokenRevealed,
}

/// The four game phases, mirrored in `card-dsl` so [`EventPattern`] can
/// name a phase without `card-dsl` depending on `game-core` (layering).
/// `game-core` maps this to its own `state::Phase` at the dispatch
/// boundary (see `engine::dispatch::trigger_scan`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    Mythos,
    Investigation,
    Enemy,
    Upkeep,
}

/// Which **timing cell** a [`Trigger::OnEvent`] ability resolves in — the
/// three-part sequence the Rules Reference runs around every triggering
/// condition. `glossary/Nested_Sequences.md`: *"Each time a triggering
/// condition occurs, the following sequence is followed: 1) execute "when..."
/// effects that interrupt that triggering condition, (2) resolve the triggering
/// condition, and then, (3) execute "after..." effects in response to that
/// triggering condition."*
///
/// **Declare the trigger word the card prints.** Reading the card is meant to
/// be enough to write the declaration; the cells are named after the words.
/// The declaration is checked against the trigger word in the card module's
/// own verbatim card-text block, and the module's prose names the cell it
/// resolves in — by reading, not by parsing. Both conventions, and why there
/// is no automated check, are in `docs/agents/standards.md` → Match a card's
/// declared `EventTiming` to its quoted trigger word.
///
/// - [`When`](Self::When) — interrupts the condition, resolving *before* its
///   impact lands. Dodge 01023's *"Fast. Play when an enemy attacks an
///   investigator at your location. / Cancel that attack."*, Cover Up 01007's
///   *"`[reaction]` When you would discover 1 or more clues at your location:
///   Discard that many clues from Cover Up instead."*
/// - [`At`](Self::At) — between the other two, after the condition's impact has
///   landed. The cell the printed words *"at"* and *"if"* name: agenda 01107's
///   *"**Forced** - At the end of the round: Place 1 doom on this agenda for
///   each `[[Ghoul]]` enemy in the Hallway or Parlor."*, Dissonant Voices 01165's
///   *"**Forced** - At the end of the round: Discard Dissonant Voices."*, and
///   for the *"if"* half act 01110's *"**Objective** - If the Ghoul Priest is
///   Defeated, advance."* — the example `glossary/If.md` itself uses.
/// - [`After`](Self::After) — once the condition has fully resolved. Most
///   reaction cards, and Silver Twilight Acolyte 01102's *"**Forced** - After
///   Silver Twilight Acolyte attacks: Place 1 doom on the current agenda."*
///
/// Within a cell, forced abilities resolve before reactions.
///
/// **A `When` declaration can be rejected**, on a triggering condition whose
/// resolve step the coordinator does not perform yet. The reject names the
/// condition, and migrating that condition — not retagging the card — is the
/// fix. What migrating costs, and why the arm is scaffolding with a terminal
/// condition, is on `ConditionResolution::Caller` in
/// `crates/game-core/src/engine/dispatch/emit.rs`; the reasoning is
/// `docs/adr/0008-a-triggering-condition-resolves-inside-its-own-sequence.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventTiming {
    /// Interrupts the triggering condition — `glossary/When.md`: *"the moment
    /// immediately after the specified timing point or triggering condition
    /// initiates, but before its impact upon the game state resolves."* The
    /// cell a printed *"when"* names, whether or not the clause says *"would"*.
    ///
    /// `glossary/Instead.md` gives this cell an internal sub-order the engine
    /// does not implement — *"'When X would occur' resolves before 'When X
    /// occurs.'"* No Core card puts both on one condition. Science Building
    /// 02056 (Dunwich) is the card that will: its *"**Forced** - When you
    /// fail a \[willpower\] test in the Science Building: Take 1 damage."*
    /// shares `SkillTestResolved`'s failure with Lucky! 01080's *"Play when
    /// you would fail a skill test"*, and as a plain `when` it must resolve
    /// after Lucky!, so a test Lucky! turns into a success deals no damage.
    /// Unmodelled, the Forced resolves first, forced-before-reaction.
    ///
    /// Rejected on a condition whose resolve step has not been migrated; see
    /// the enum's own docs.
    When,
    /// Resolves between `when` and `after` abilities with the same triggering
    /// condition — `glossary/At.md` (identical in `glossary/If.md`): *"These
    /// abilities trigger in between any "when..." abilities and any "after..."
    /// abilities with the same triggering condition."* — and, per ADR 0008's
    /// interpretation, after the condition's own impact has landed.
    ///
    /// Since #702 this cell is reachable on **every** triggering condition, not
    /// just the round end as before — the enemy attack and its soak window, the
    /// last two conditions to bypass the coordinator entirely, joined the walk
    /// in #704 — and regardless of who owns the resolve step: caller-owned costs
    /// the `when` cell only.
    At,
    /// Resolves once the triggering condition has finished — `glossary/After.md`:
    /// *"the moment immediately after the specified timing point or triggering
    /// condition has fully resolved."*
    ///
    /// A coordinator-owned condition prevented in its `when` cell never fully
    /// resolves, and takes the **rest of its sequence** with it: neither this
    /// cell nor [`At`](Self::At) runs, and the remainder of the `when` cell is
    /// withdrawn too (#714; `data/arkhamdb-faq/core/01023.md`).
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
    /// Choose and discard 1 card from the controller's hand (Wendy Adams
    /// 01005). Payable only while the hand holds a card that is not a
    /// weakness — Dark Memory 01013's ruling: *"You cannot choose to discard a
    /// Weakness card from your hand, neither for exceeding the max number of
    /// cards in hand, nor for card effects that make you 'choose and
    /// discard'."* (<https://arkhamdb.com/card/01013>). Paid through a choice
    /// of hand card before the effect resolves, on a triggered ability or a
    /// `[fast]` activated one; an `[action]` ability with the cost is refused
    /// (TODO(#1031)).
    DiscardCardFromHand,
    /// Spend `count` tokens of the named [`UseKind`]
    /// from the source asset's runtime uses-pool (".38 Special": "Spend 1
    /// ammo"). Insufficient remaining of that kind rejects the activation.
    SpendUses {
        /// Which uses-kind to spend (Ammo, Charges, …).
        kind: UseKind,
        /// How many to spend.
        count: u8,
    },
    /// Discard the source card from play to pay for its own ability
    /// (Beat Cop 01018, Knife 01086), wherever it sits, to its owner's pile.
    /// Distinct from [`Effect::DiscardSelf`], which discards the source as
    /// the ability's effect. Must be the only source-referencing cost on an
    /// ability (it removes the source); paid last.
    DiscardSelf,
    /// The controller takes `damage` damage and `horror` horror (#1029). One
    /// variant carries both kinds because the snapshot prints both, on three
    /// cards that agree on the shape:
    ///
    /// - Forbidden Knowledge 01058: *"\[fast\] Exhaust Forbidden Knowledge and
    ///   take 1 horror: Move 1 secret from Forbidden Knowledge to your resource
    ///   pool, as a resource."*
    /// - Smoking Pipe 02116: *"\[fast\] Spend 1 supply, exhaust Smoking Pipe,
    ///   and take 1 damage: Heal 1 horror."*
    /// - Painkillers 02117: *"\[fast\] Spend 1 supply, exhaust Painkillers, and
    ///   take 1 horror: Heal 1 damage."*
    ///
    /// Always payable: you can always take harm, and activating into a lethal
    /// cost is legal. The harm is dealt like any other — offered to soakers,
    /// then placed — and resolves, with every nested sequence it opens, before
    /// the ability's effect, which is skipped if the harm eliminated the
    /// controller. Refused on an `[action]` ability: there the harm would have to
    /// resolve before the attacks of opportunity, which the take-an-action step
    /// does not do yet.
    ///
    /// Distinct from harm dealt *to the source* as a cost (Beat Cop 01028's
    /// *"deal 1 damage to it"*), which this does not express.
    TakeHarm {
        /// Damage the controller takes.
        damage: u8,
        /// Horror the controller takes.
        horror: u8,
    },
}

// ---- usage limits ----------------------------------------------

/// A "Limit X per \[period\]" cap on how often an ability may fire.
///
/// Per the Rules Reference page 14: *"Each instance of an ability with
/// such a limit may be initiated X times during the designated period.
/// If a card leaves play and re-enters play during the same period,
/// the card is considered to be bringing a new instance of the ability
/// to the game."* The same entry adds *"Unless stated otherwise, limits are
/// player specific."*, so the count is kept per instance **and** per
/// triggering investigator: each investigator who can reach the ability gets
/// their own X (The Stranger 03047a's ruling,
/// <https://arkhamdb.com/card/03047a>).
///
/// Canonical motivating card: Roland Banks (01001) —
/// `[reaction] After you defeat an enemy: Discover 1 clue at your
/// location. (Limit once per round.)` compiles to
/// `UsageLimit { count: 1, period: UsagePeriod::Round }`.
///
/// Storage of the per-instance counter lives on
/// `CardInPlay` (in `game_core::state`): see
/// `ability_usage` (its per-instance, per-investigator counter map). When a
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
/// `Round` (Roland's "Limit once per round"), `Phase` (Agnes Baker's
/// "Limit once per phase"), `Turn` ("Skids" O'Toole's "Limit once per
/// turn") and `SkillTest` (Wendy Adams's "Limit once per test/ability").
/// `Game` ("limit once per game" — group or player) lands when the
/// first consumer appears.
///
/// The period decides when a counter resets, and nothing else: the engine's
/// usage record notes the round, the phase **and** the turn of its last fire
/// either way, and each period compares the part of that clock it is measured
/// over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsagePeriod {
    /// A game round, as defined by the framework: begins at 1.1 Mythos,
    /// ends at 4.6 Upkeep (Rules Reference page 23). Counter resets
    /// when `GameState::round` (in `game_core::state`)
    /// advances — and not at a phase change within the round.
    Round,
    /// A game phase (Mythos, Investigation, Enemy, Upkeep). Counter resets
    /// when either `GameState::phase` or `GameState::round` changes, so the
    /// Investigation phase of one round is a different period from the
    /// Investigation phase of the next.
    Phase,
    /// An investigator's turn. Counter resets when the round, the phase or
    /// the active investigator changes: each investigator takes one turn per
    /// Investigation phase, so those three name the turn. Outside the
    /// Investigation phase there is no turn, and the period is the rest of the
    /// phase.
    Turn,
    /// A skill test. Counter resets as each test starts: the engine's usage
    /// record notes the in-flight test's identity, and a new test has a new
    /// one.
    ///
    /// Wendy Adams 01005 prints *"(Limit once per test/ability.)"*: a chaos
    /// token revealed by an ability outside any skill test is the
    /// *"/ability"* half. No corpus card reveals a token outside a test, so
    /// that half is unreachable and not modelled; outside a test every reveal
    /// would fall in the one period.
    SkillTest,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Ability {
    pub trigger: Trigger,
    /// The **bold action designator** the ability prints, if any — `None` for
    /// an ability that prints none. It performs the designated action, carrying
    /// the modification the text describes; see [`ActionDesignator`], which
    /// also says why it lives here rather than on the trigger.
    ///
    /// Only a [`Trigger::Activated`] or [`Trigger::OnPlay`] ability carries
    /// one, and only [`activated_as`] and [`on_play_as`] set it. Stays
    /// implicitly optional on the wire, like `usage_limit`.
    pub designator: Option<ActionDesignator>,
    /// Payment costs (besides action cost). Empty for abilities with no
    /// non-action payment. Paid when an activated or a reaction ability
    /// initiates; a forced ability's are neither checked nor paid. Required on
    /// the wire (#453).
    pub costs: Vec<Cost>,
    pub effect: Effect,
    /// "Limit X per \[period\]" cap. `None` for abilities with no
    /// printed cap. Stays implicitly optional (#453 per-field reassessment):
    /// serde treats a missing `Option` field as `None`, the genuine
    /// absent-by-design case.
    pub usage_limit: Option<UsageLimit>,
    /// A card-supplied [`Condition`] the initiation gate checks for a **play
    /// restriction the engine can't derive itself** — the Rules Reference's
    /// *"specific conditions that must be true in order to use them"*
    /// (`glossary/Play_Restrictions_Permissions_and_Instructions.md`). While it
    /// is false the ability is not offered, a forced one does not initiate, and
    /// a played card is not played.
    ///
    /// A card supplies one for one of two reasons:
    ///
    /// - **an opaque native effect the change-state check can't see into.** The
    ///   engine asks whether an effect has the potential to change the game
    ///   state, but an [`Effect::Native`] is a black box to it: Cover Up 01007's
    ///   *"if there are any clues on Cover Up"*, and The Barrier 01109's
    ///   Hallway group affording its clue spend.
    /// - **a trigger narrowing the event pattern can't yet express.** Lita
    ///   Chantler 01117's *"an investigator at your location successfully
    ///   attacks a \[\[Monster\]\] enemy"* — a misfit, kept here until the
    ///   pattern grows the scope fields it needs (her module doc says which).
    ///
    /// A "would it do anything" case is **not** one: Agnes Baker 01004,
    /// Pickpocketing 01046, Scavenging 01073, Rabbit's Foot 01075 and Heirloom
    /// of Hyperborea 01012 belong to the change-state check, which reads the
    /// effect itself.
    ///
    /// Evaluated like any other condition, so a declarative one needs no Rust;
    /// [`Condition::Native`] is the escape hatch for a gate the vocabulary
    /// can't express. `None` for the common no-gate case; stays implicitly
    /// optional on the wire (#453 per-field carve-out, like `usage_limit`).
    pub eligibility: Option<Condition>,
    /// Where the card must be for this ability to interact with the game.
    /// [`AbilityZone::InPlay`] for every ability [`Self::with_active_in`] does
    /// not move; required on the wire (#453).
    pub active_in: AbilityZone,
}

/// The zone a card's ability is live in — see [`Ability::active_in`].
///
/// `glossary/Ability.md`: *"Card abilities only interact with the game if the
/// card bearing the ability is in play, unless the ability (or rules for the
/// cardtype) specifically references its use from an out-of-play area."* The
/// default is the rule; a card whose ability names an out-of-play area says so
/// with [`Ability::with_active_in`]. The forced trigger scan reads a zone's
/// cards only for the abilities live there. A Fast event's reaction is not read
/// through this: it is played from hand, a rule of the event cardtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AbilityZone {
    /// On the board: in a play area, a threat area, at a location, or the
    /// current act or agenda.
    InPlay,
    /// In the hand of the investigator holding the card — Dark Memory 01013's
    /// *"If Dark Memory is in your hand at the end of your turn"*.
    Hand,
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

    /// Attach the costs paid to initiate this ability (see [`Self::costs`]),
    /// for a triggered ability: the activated constructors take theirs as an
    /// argument. Builder-style sugar, chainable off the `on_event(...)`
    /// constructors — Pickpocketing 01046's *"exhaust Pickpocketing"*.
    #[must_use]
    pub fn with_costs(mut self, costs: Vec<Cost>) -> Self {
        self.costs = costs;
        self
    }

    /// Attach an eligibility condition (see [`Self::eligibility`]).
    /// Builder-style sugar, chainable off the `on_event(...)` constructors.
    #[must_use]
    pub fn with_eligibility(mut self, condition: Condition) -> Self {
        self.eligibility = Some(condition);
        self
    }

    /// Make this ability live in `zone` rather than in play (see
    /// [`Self::active_in`]), for an ability that names its use from an
    /// out-of-play area. Builder-style sugar, chainable off the `on_event(...)`
    /// constructors.
    #[must_use]
    pub fn with_active_in(mut self, zone: AbilityZone) -> Self {
        self.active_in = zone;
        self
    }

    /// Every native tag this ability references, each with the registry
    /// namespace it resolves in — the `cards` crate's corpus test checks each
    /// one is registered (#986).
    ///
    /// Walks the whole ability: its own eligibility, its action designator,
    /// the trigger's elder-sign outcome, and the effect tree, descending
    /// into every [`Effect`], [`Condition`] and [`IntExpr`] it holds,
    /// granted abilities included. The matches carry no wildcard arms, so a
    /// new variant does not compile until someone decides whether it can
    /// carry a tag. Order follows the walk; a tag referenced twice is
    /// reported twice.
    #[must_use]
    pub fn native_refs(&self) -> Vec<NativeRef<'_>> {
        let mut refs = Vec::new();
        self.collect_native_refs(&mut refs);
        refs
    }

    fn collect_native_refs<'a>(&'a self, refs: &mut Vec<NativeRef<'a>>) {
        if let Some(eligibility) = &self.eligibility {
            eligibility.collect_native_refs(refs);
        }
        match &self.designator {
            None
            | Some(
                ActionDesignator::Move
                | ActionDesignator::Parley
                | ActionDesignator::Resign
                | ActionDesignator::AutomaticEvade,
            ) => {}
            Some(ActionDesignator::Evade {
                skill_override: _,
                reveal_clause,
                on_success,
            }) => {
                if let Some(clause) = reveal_clause {
                    clause.effect.collect_native_refs(refs);
                }
                if let Some(effect) = on_success {
                    effect.collect_native_refs(refs);
                }
            }
            Some(ActionDesignator::Fight {
                combat_modifier,
                extra_damage,
                skill_override: _,
                reveal_clause,
            }) => {
                combat_modifier.collect_native_refs(refs);
                extra_damage.collect_native_refs(refs);
                if let Some(clause) = reveal_clause {
                    clause.effect.collect_native_refs(refs);
                }
            }
            Some(ActionDesignator::Investigate {
                shroud_modifier,
                instead_of_discovery,
            }) => {
                shroud_modifier.collect_native_refs(refs);
                if let Some(effect) = instead_of_discovery {
                    effect.collect_native_refs(refs);
                }
            }
        }
        match &self.trigger {
            // None of these holds an effect, condition or integer expression: an
            // event pattern names what happened, never a condition on it.
            Trigger::Constant
            | Trigger::OnPlay
            | Trigger::OnCommit
            | Trigger::Revelation
            | Trigger::OnSkillTestResolution { .. }
            | Trigger::OnEvent { .. }
            | Trigger::Activated { .. } => {}
            Trigger::ElderSign { outcome } => outcome.collect_native_refs(refs),
        }
        self.effect.collect_native_refs(refs);
    }
}

/// Which registry namespace a native tag resolves in. The two are separate
/// slots of `game_core::card_registry::CardRegistry`, so a tag registered in
/// the wrong one is never found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeKind {
    /// An [`Effect::Native`], resolved by `native_effect_for`.
    Effect,
    /// A [`Condition::Native`], resolved by `native_condition_for`.
    Condition,
}

/// One native tag an [`Ability`] references — see [`Ability::native_refs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeRef<'a> {
    pub kind: NativeKind,
    pub tag: &'a str,
}

impl TokenOutcome {
    fn collect_native_refs<'a>(&'a self, refs: &mut Vec<NativeRef<'a>>) {
        self.modifier.collect_native_refs(refs);
        for effect in [&self.immediate, &self.on_success, &self.on_fail]
            .into_iter()
            .flatten()
        {
            effect.collect_native_refs(refs);
        }
    }
}

impl Effect {
    fn collect_native_refs<'a>(&'a self, refs: &mut Vec<NativeRef<'a>>) {
        match self {
            Effect::Native { tag } => refs.push(NativeRef {
                kind: NativeKind::Effect,
                tag,
            }),
            Effect::Deal { amount, .. } => amount.collect_native_refs(refs),
            Effect::PlaceDoomOnCurrentAgenda { count, .. } | Effect::DrawCards { count, .. } => {
                count.collect_native_refs(refs);
            }
            Effect::Seq(effects) => {
                for effect in effects {
                    effect.collect_native_refs(refs);
                }
            }
            Effect::If {
                condition,
                then,
                else_,
            } => {
                condition.collect_native_refs(refs);
                then.collect_native_refs(refs);
                if let Some(else_) = else_ {
                    else_.collect_native_refs(refs);
                }
            }
            Effect::ForEach { body, .. } => body.collect_native_refs(refs),
            Effect::ChooseOne(branches) => {
                for branch in branches {
                    branch.effect.collect_native_refs(refs);
                }
            }
            Effect::SkillTest {
                on_success,
                on_fail,
                ..
            } => {
                for effect in [on_success, on_fail].into_iter().flatten() {
                    effect.collect_native_refs(refs);
                }
            }
            Effect::Grant {
                condition,
                abilities,
                ..
            } => {
                if let Some(condition) = condition {
                    condition.collect_native_refs(refs);
                }
                for ability in abilities {
                    ability.collect_native_refs(refs);
                }
            }
            Effect::Restrict(Restriction::CannotLeavePlay { while_ }) => {
                while_.collect_native_refs(refs);
            }
            Effect::GainResources { .. }
            | Effect::DiscoverClue { .. }
            | Effect::DealDamageToEnemy { .. }
            | Effect::Disengage { .. }
            | Effect::AutoEvade { .. }
            | Effect::Heal { .. }
            | Effect::Modify { .. }
            | Effect::AutoResolve { .. }
            | Effect::AdvanceCurrentAct
            | Effect::ReachResolution(_)
            | Effect::DiscardSelf
            | Effect::Cancel
            | Effect::RevealChaosToken
            | Effect::PutIntoThreatArea { .. }
            | Effect::BoostAttackDamage(_)
            | Effect::DiscoverAdditionalClues(_)
            | Effect::Restrict(
                Restriction::CannotPlay(_)
                | Restriction::ExtraActionCost { .. }
                | Restriction::EnemyMovementBlocked { .. }
                | Restriction::InvestigatorMovementBlocked
                | Restriction::CannotAttack { .. },
            )
            | Effect::SearchDeck { .. }
            | Effect::AttachSelfToLocation
            | Effect::TakeControl { .. }
            | Effect::GainAdditionalAction { .. }
            | Effect::AdditionalActionWhileInPlay { .. }
            | Effect::AdditionalSlots { .. }
            | Effect::TreatRevealedToken { .. }
            | Effect::PlaceEventsOnBottomOfDeck
            | Effect::PlayTopmostEventInDiscardPile => {}
        }
    }
}

impl Condition {
    fn collect_native_refs<'a>(&'a self, refs: &mut Vec<NativeRef<'a>>) {
        match self {
            Condition::Native { tag } => refs.push(NativeRef {
                kind: NativeKind::Condition,
                tag,
            }),
            Condition::SkillTest { .. }
            | Condition::SkillTestKind(_)
            | Condition::Compare { .. }
            | Condition::ControlStatus { .. } => {}
        }
    }
}

impl IntExpr {
    fn collect_native_refs<'a>(&'a self, refs: &mut Vec<NativeRef<'a>>) {
        match self {
            IntExpr::Cond { when, .. } => when.collect_native_refs(refs),
            IntExpr::Lit(_) | IntExpr::Count(_) => {}
        }
    }
}

// ---- effects ---------------------------------------------------

/// One alternative of an [`Effect::ChooseOne`]: the effect, and the label the
/// controller reads when picking it.
///
/// The label is authored from the card's printed text. Where the printing is
/// already a list of options — What Have You Done? 01110's two bullets — it is
/// copied verbatim, `(→R1)` / `(→R2)` markers included, since a player choosing
/// an ending should see which ending it is. Where the printing is one sentence
/// covering both options — First Aid 01019's *"Heal 1 damage or horror from an
/// investigator at your location."* — labelling means **splitting** it, so the
/// card's module doc quotes the full printed sentence directly above the
/// derived labels and the split stays auditable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChoiceBranch {
    /// What the controller reads when offered this branch.
    pub label: String,
    /// What resolves if they pick it.
    pub effect: Effect,
}

/// What an ability does when it resolves.
///
/// Effects compose: [`Effect::Seq`] runs a list in order,
/// [`Effect::If`] branches, [`Effect::ForEach`] applies a body once
/// per resolved target, [`Effect::ChooseOne`] presents alternatives
/// to the controller.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Deal `amount` of `kind` (damage or horror) to the resolved target
    /// investigator, applying defeat if the new total reaches their max health
    /// (damage) or max sanity (horror). `amount == 0` is a no-op (no event, no
    /// target resolution). Built via the [`deal_damage`] / [`deal_horror`] sugar.
    Deal {
        kind: HarmKind,
        target: InvestigatorTarget,
        amount: IntExpr,
    },
    /// Deal `amount` direct (non-test) damage to the resolved enemy
    /// `target`, applying the defeat cascade (Beat Cop 01018). Typed (not
    /// `Native`) so the activation pre-cost check can verify ≥1 candidate
    /// before any cost is paid. `amount == 0` is a no-op.
    DealDamageToEnemy { target: EnemyTarget, amount: u8 },
    /// Disengage the controller from the resolved enemy `target`. It stays
    /// where it is, and stays ready: Elusive 01050's ruling, *"Disengaging does
    /// not exhaust an enemy."* (<https://arkhamdb.com/card/01050>). A no-op on
    /// an enemy not engaged with the controller — unengaged, or engaged with
    /// another investigator.
    Disengage { target: EnemyTarget },
    /// **Automatically evade** the resolved enemy `target` (`GLOSSARY.md`,
    /// "Automatic evasion"): no skill test, so not a *successful* evasion,
    /// but an evasion all the same. `glossary/Evade_Action.md`: *"If an
    /// ability "automatically" evades 1 or more enemies, no skill test is made
    /// for the evasion attempt."* and *"Any time an enemy is evaded (whether by
    /// an evade action, or by card ability), the enemy is exhausted (if it was
    /// ready) and the engagement is broken."* The enemy need not be engaged
    /// with the evader (the 01076 ruling, <https://arkhamdb.com/card/01076>),
    /// and "after you evade" reactions still see it (Pickpocketing 01046's
    /// ruling, <https://arkhamdb.com/card/01046>).
    ///
    /// Stray Cat 01076 (a chosen non-Elite enemy at your location) and
    /// Cunning Distraction 01078 (each enemy at your location). Typed, not
    /// `Native`, so Stray Cat's target is checked before its cost is paid.
    AutoEvade { target: EnemyTarget },
    /// Heal `count` of `kind` (damage or horror) from the resolved target
    /// investigator — the inverse of [`Effect::Deal`] (no defeat
    /// interaction). Heals at most the current amount (saturating at 0).
    /// `count == 0`, or a target with nothing to heal, is a no-op.
    ///
    /// **A bare "Heal X" is not a choice.** When a card says only *"Heal 1
    /// damage"*, `target` is the controller — [`InvestigatorTarget::You`] —
    /// and never `chosen_anywhere()`. The official FAQ:
    ///
    /// > "Heal X damage/horror" is shorthand for "Heal X damage/ horror from
    /// > your investigator." If a card simply reads "Heal X horror" or "Heal
    /// > X damage," you can only use it to heal horror or damage from your
    /// > investigator. Cards that allow you to heal other investigators or
    /// > assets will specify that.
    ///
    /// (`data/official-faq/Frequently_Asked_Questions.md`.) A wider target is
    /// legitimate only where the card prints the wider wording — First Aid
    /// 01019's *"Heal 1 damage or horror from an investigator at your
    /// location."* Smoking Pipe 02116 (*"Spend 1 supply, exhaust Smoking Pipe,
    /// and take 1 damage: Heal 1 horror."*) and Painkillers 02117 (*"…take 1
    /// horror: Heal 1 damage."*) print the bare form, so both encode `You`.
    Heal {
        kind: HarmKind,
        target: InvestigatorTarget,
        count: u8,
    },
    /// Adjust a stat by `delta`, for the audience described by
    /// `audience` and the duration described by `scope`. Most scopes are
    /// passive contributions to engine queries rather than mutations of
    /// the target's stored fields.
    Modify {
        stat: Stat,
        delta: i8,
        scope: ModifierScope,
        /// Who receives the adjustment. [`ModifierAudience::Controller`]
        /// for the "You get …" majority; the other variants let a card
        /// reach past its own controller.
        audience: ModifierAudience,
    },
    /// Latch the in-flight skill test's [`Determination`]: the test
    /// automatically fails, or automatically succeeds, whatever the
    /// numbers say.
    ///
    /// `data/rules-reference/rules/glossary/Automatic_Failure_Success.md`:
    ///
    /// > Some card or token abilities may cause a skill test to
    /// > automatically fail or to automatically succeed.
    ///
    /// One variant carrying the two-valued determination rather than two
    /// variants with one consumer each. It is not a job for
    /// [`Native`](Self::Native): automatic failure and success are Rules
    /// Reference concepts with their own glossary entry, and the engine has
    /// to understand them whoever declares them — the substitution they
    /// name lands in the modified-value fold, not in card-local Rust.
    ///
    /// **No window list.** The moment a determination is latched comes from
    /// the declaring card's own trigger, exactly as
    /// [`ModifierScope::ThisSkillTest`] already works, so the evaluator
    /// enumerates no legal windows. The snapshot's latch range is wider
    /// than "before ST.3" — Possession 03340 latches on commit at ST.2,
    /// Delusory Evils 52065 reacts at ST.6, and neither is in the corpus
    /// yet. The one gate is that a test must be **in
    /// flight**: with none there is no skill-test identity to stamp onto
    /// the recorded row, so the effect is refused rather than banked (the
    /// same argument [`Modify`](Self::Modify) with
    /// [`ModifierScope::ThisSkillTest`] scope makes).
    AutoResolve {
        /// Which determination the card declares.
        determination: Determination,
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
    /// the body can refer to it (as [`InvestigatorTarget::Each`]).
    ForEach {
        targets: ForEachTargets,
        body: Box<Effect>,
    },
    /// Present alternatives to the controller. Resolves to the chosen
    /// branch's effect. Requires an `AwaitingInput` round-trip.
    ///
    /// **Each branch carries the label the controller reads** — a
    /// [`ChoiceBranch`], not a bare `Effect`. Through #775 the evaluator
    /// rendered `format!("{effect:?}")`, so What's Going On?! 01105 offered the
    /// lead investigator `Native { tag: "01105:random-discard-each" }`. A label
    /// is authored per card, from the printed text, and there is no position in
    /// which a branch does not need one — hence a mandatory `String` on the
    /// branch rather than an `Option`, or an `Effect::Labeled` wrapper node
    /// that would compile anywhere and mean something in one place.
    ChooseOne(Vec<ChoiceBranch>),
    /// Advance the current act one step: the cursor moves and the act's
    /// on-advance reverse fires. Used by act objectives like 01110 ("If the
    /// Ghoul Priest is Defeated, advance."). A *terminal* act advances the
    /// same way — its reverse is what ends the scenario, via
    /// [`ReachResolution`](Self::ReachResolution).
    AdvanceCurrentAct,
    /// Reach the printed resolution point `(→R#)`, ending the scenario there.
    ///
    /// Rules Reference, `Winning and Losing`: *"Some instructions in the act
    /// deck (as well as on other encounter cardtypes) contain resolution
    /// points, in the format of: '**(→R#)**.'"* A terminal card's reverse
    /// prints one, and running this effect is how the card reaches it — the
    /// same way a reverse does anything else printed on it. The Gathering's
    /// act 3 (01110) and agenda 3 (01107) are the two consumers in the corpus
    /// today, and every future scenario's terminal cards are behind them.
    ///
    /// **A bare `u8`, not the engine's `ResolutionId`.** `card-dsl` has no
    /// workspace dependencies and that newtype lives in `game-core`, so the
    /// conversion happens where the effect is evaluated. The number is the
    /// printed one: the campaign guide titles its entries "Resolution 1",
    /// "Resolution 2", and so on.
    ///
    /// Latching is first-writer-wins (the engine's `end_scenario`), so a
    /// resolution point already reached this scenario stands — see
    /// `docs/adr/0004-a-latched-resolution-cancels-opportunities-not-resolutions.md`.
    /// The decision to make this an effect rather than a card datum is
    /// `docs/adr/0013-a-resolution-point-is-a-printed-effect.md`.
    ReachResolution(u8),
    /// Place `count` doom on the current agenda, and — only if `may_advance` —
    /// run the doom-threshold check.
    ///
    /// Six places in the corpus print *"place N doom on the current agenda"*:
    /// Ancient Evils 01166, Silver Twilight Acolyte 01102, Dark Memory 01013,
    /// Offer of Power 01178, the back of act Saracenic Script 02240, and Blood
    /// on the Altar 02195's `[elder_thing]` token. **Only three of them also
    /// print the advance clause** — 01166 and 01013 as *"This effect can cause
    /// the current agenda to advance."*, 01178 parenthetically as *"(this
    /// effect can cause the current agenda to advance)"*. 01102, 02240, and
    /// 02195 print the placement alone, and that difference is `may_advance`.
    ///
    /// `data/rules-reference/rules/glossary/Doom.md`:
    ///
    /// > If there are no "**Objective** – " requirements for advancing the
    /// > current agenda and the requisite amount of doom is in play (among the
    /// > agenda and all cards in play), the agenda advances during the "Check
    /// > doom threshold" step of the Mythos phase. Unless a card otherwise
    /// > specifies that it can advance the agenda, this is the only time at
    /// > which the agenda can advance.
    ///
    /// The printed clause **is** that "otherwise specifies" — which is why the
    /// three cards that carry it bother to print a sentence the other three
    /// omit. So doom placed by 01102's attack sits on the agenda and waits for
    /// Mythos step 1.3 even if it tips the threshold, while doom placed by
    /// 01166's Revelation can advance the agenda then and there.
    ///
    /// When the check does run, it runs **once, after all `count` doom is
    /// placed** — Offer of Power 01178's *"place 2 doom"* is one placement of
    /// two, not two placements of one, so a threshold reached by the first
    /// cannot advance the agenda out from under the second.
    ///
    /// `count` is an [`IntExpr`] rather than a `u8` because the clause is
    /// already printed with two different numbers in the corpus (01166's 1,
    /// 01178's 2) and out of corpus with a computed one (Jeremiah Pierce
    /// 50044's *"for each point you fail by"*). A `count` that evaluates to
    /// zero or negative is a **full no-op** — no placement and no threshold
    /// check, so it cannot advance an already-at-threshold agenda.
    ///
    /// Typed rather than [`Native`](Self::Native) (graduated from two
    /// card-local tags in #716) so it can sit inside a [`Seq`](Self::Seq),
    /// [`ChooseOne`](Self::ChooseOne), or [`If`](Self::If) as a sub-effect —
    /// which the three remaining corpus printings each need: 01178 inside a
    /// `ChooseOne` branch, 02240 as the last step of a `Seq`, and 02195 under
    /// an `If`-on-failure in a chaos-token effect.
    PlaceDoomOnCurrentAgenda {
        /// How much doom to place. Evaluated once, at resolution.
        count: IntExpr,
        /// Whether the card prints *"this effect can cause the current agenda
        /// to advance"*. Only then is the doom threshold checked here; without
        /// it the placement waits for Mythos step 1.3, per `glossary/Doom.md`.
        may_advance: bool,
    },
    /// A card-local Rust effect, resolved by tag through the host's
    /// `CardRegistry.native_effect_for`. The generic escape hatch for
    /// single-use card logic that doesn't earn a shared `Effect` variant
    /// (see issue #276). The `cards` crate maps the tag to a Rust fn; the
    /// evaluator rejects loudly on an unknown tag or absent registry.
    ///
    /// **A pattern a second card wants graduates to a variant**
    /// (`docs/agents/standards.md`). *"Place N doom on the current agenda"* was
    /// the first to do so: Ancient Evils 01166 and Silver Twilight Acolyte 01102
    /// each carried a byte-identical `<code>:place-doom` tag until #716 replaced
    /// both with [`PlaceDoomOnCurrentAgenda`](Self::PlaceDoomOnCurrentAgenda).
    /// Reach for that variant, not a fresh tag.
    Native { tag: String },
    /// Initiate a skill test as part of a card effect (treachery
    /// Revelation, agenda forced effect, …). The evaluator maps `skill`
    /// to the engine's `SkillKind` and runs the test against
    /// `difficulty`, always suspending at the commit window. `on_fail`
    /// runs after the test resolves **on failure**, with the failure
    /// margin available via the evaluator context's `failed_by` (success
    /// is a no-op for the cards in scope). See issue #286.
    ///
    /// **This variant is one of the two ways a card prompts a test, and the
    /// rules draw the same line.** The official FAQ, defining *"a skill test on
    /// a card"*: *"any ability that directly prompts a skill test, either
    /// through the template 'test skill (X),' or by initiating an action that
    /// is, in itself, a skill test (for example, any card with Fight, Evade, or
    /// Investigate action designators)."*
    /// (`data/official-faq/Frequently_Asked_Questions.md`.) The first clause is
    /// this variant; the second is [`ActionDesignator`], where the test comes
    /// from the action rather than from an explicit instruction. Anything that
    /// eventually needs to ask *which card a test is on* — no Core or Dunwich
    /// card does — must accept both, not just this one.
    SkillTest {
        skill: SkillKind,
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
    /// Discard the firing card instance (the evaluator context's
    /// `source`) from play, wherever it sits, to its owner's pile: a
    /// treachery to the encounter discard, a played Barricade 01038 to its
    /// player's. Used by persistent treacheries' `Forced` self-discard
    /// abilities (Frozen in Fear 01164, Dissonant Voices 01165, Obscuring Fog
    /// 01168). An enemy source discards the enemy itself, by its owner and
    /// without defeating it (Mob Enforcer 01101's *"Discard Mob Enforcer."*).
    /// Rejects if there is no source or it isn't in play.
    DiscardSelf,
    /// Cancel the current cancellable game impact — the subject of the
    /// `when`-cell window this effect resolves inside. Flags that triggering
    /// condition as prevented (the engine's `EmitEventFrame::prevented`, #1055),
    /// and its timing coordinator skips the prevented impact at its resolve step
    /// (an enemy attack's damage/horror — Dodge 01023; or a clue discovery —
    /// Cover Up 01007). The flag is the condition's own, so a condition the same
    /// effect emits after the cancel still resolves.
    /// RR p.6: the cancelled thing is "still regarded as initiated", only
    /// its effects are prevented. (Axis D #336.)
    ///
    /// A cancelled triggering condition takes **the rest of its sequence** with
    /// it: the `at` and `after` cells do not run, and neither does the rest of
    /// the `when` cell (#714). Dodge 01023's ruling is the citation, quoted on
    /// the engine's flag (`EmitEventFrame::prevented`), which every
    /// coordinator-owned condition reads.
    ///
    /// Cancel is the degenerate replacement ("replace with nothing"): a card
    /// that replaces with its own effect runs that effect then `Cancel`
    /// (Cover Up = `Seq[discard-from-self, Cancel]`). Both suppress the rest of
    /// the sequence, so the engine models them as one signal.
    /// TODO(#366): a true replace-with-a-different-impact effect — the one
    /// replacement whose condition still resolves, and whose later cells
    /// therefore presumably still run.
    Cancel,
    /// Reveal a new chaos token for the skill test in flight — Wendy Adams
    /// 01005's *"Cancel that chaos token and return it to the bag. Reveal a
    /// new chaos token."*, declared as `Seq[Cancel, RevealChaosToken]` in the
    /// `when` cell of [`EventPattern::ChaosTokenRevealed`].
    ///
    /// It sets the test back to its draw step, so once the cancelled reveal's
    /// sequence is abandoned the test draws again and announces the new token
    /// in a fresh `when` cell. *"Return it to the bag"* needs nothing: a drawn
    /// token never leaves the bag, so the redraw can draw the same token.
    /// Rejected with no test in flight. Wendy is its only consumer, a
    /// spec-approved exception to `standards.md`'s DSL-primitive threshold
    /// (#780).
    RevealChaosToken,
    /// Put the card with this printed `code` into the controller's threat
    /// area as a fresh in-play instance. The Revelation of persistent
    /// threat-area treacheries (Frozen in Fear 01164, Dissonant Voices
    /// 01165) and of asset weaknesses that enter play there (The
    /// Necronomicon 01009) — the card names its own `CODE`. (Attaching to a *location*
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
        /// Horror to seed on the placed instance ("with 3 horror on it",
        /// The Necronomicon 01009). `0` for cards that enter without horror.
        /// The horror is *placed* as the card enters play, not dealt, so no
        /// harm event fires for it.
        horror: u8,
    },
    /// Draw `count` cards for the resolved target investigator —
    /// "draw 1 card" (Guts 01089, Perception 01090, Overpower 01091,
    /// Manual Dexterity 01092), or a count read off state as the effect
    /// resolves: Daisy Walker 01002's *"draw 1 card for each \[\[Tome\]\]
    /// you control"* is [`IntExpr::Count`] of [`Quantity::ControlledCards`].
    /// A count that evaluates to 0 or below is a no-op. Deck-out (drawing
    /// from an empty deck) follows the engine's existing `draw_cards`
    /// behavior; the elimination consequence is out of this primitive's
    /// scope.
    DrawCards {
        target: InvestigatorTarget,
        count: IntExpr,
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
    /// Add `N` to the in-flight skill test's bonus clue count — Deduction
    /// 01039's "discover 1 **additional** clue at that location."
    /// Accumulated at commit time (under [`Trigger::OnCommit`]) onto the
    /// in-flight record; **only an Investigate skill test's follow-up reads
    /// it**, so the "while investigating" qualifier is intrinsic, as is "if
    /// successful" (that follow-up discovers only on success). A no-op when
    /// there is no in-flight test.
    ///
    /// It raises that one discovery's count rather than making a second
    /// discovery — a card-text distinction, not an implementation detail; see
    /// the **Discovery** entry in `GLOSSARY.md` and Deduction's module doc for
    /// the FAQ that settles it.
    DiscoverAdditionalClues(u8),
    /// A constant restriction the source card imposes while in play
    /// (under [`Trigger::Constant`]). **Inspected, not executed** — the
    /// engine reads it at the relevant decision point (`play_is_prohibited`
    /// for `CannotPlay`, `pending_action_surcharge` for `ExtraActionCost`);
    /// resolving it as an effect is a misuse and rejects.
    Restrict(Restriction),
    /// **An additional action for as long as the source card is in play**,
    /// once during each of its controller's turns (see **Standard action /
    /// additional action** in `GLOSSARY.md`): Leo De Luca 01048's *"You may
    /// take an additional action during your turn."* A constant marker like
    /// [`Restrict`](Self::Restrict), under a [`Trigger::Constant`]:
    /// **inspected, not executed** — the engine reads it off the card in play
    /// each time it works out the investigator's actions, so the grant is gone
    /// the moment the card leaves play. Resolving it rejects.
    ///
    /// The executed form, a grant an ability resolves, is
    /// [`GainAdditionalAction`](Self::GainAdditionalAction). That a
    /// constant-only marker is an `Effect` at all, which the evaluator must
    /// refuse if executed, is #1085.
    AdditionalActionWhileInPlay {
        /// What the additional action may be used for. `None` is any action.
        restriction: Option<AdditionalActionRestriction>,
    },
    /// **Additional slots for as long as the source card is in play**:
    /// Daisy's Tote Bag 01008's *"You have 2 additional hand slots, which can
    /// only be used to hold \[\[Tome\]\] assets."* A constant marker like
    /// [`AdditionalActionWhileInPlay`](Self::AdditionalActionWhileInPlay),
    /// under a [`Trigger::Constant`]: **inspected, not executed** — the engine
    /// reads it off the investigator's cards each time it works out their slot
    /// capacity, so the slots are gone the moment the card leaves play.
    /// Resolving it rejects.
    ///
    /// Each grant is its own pool of `count` slots of type `slot`; `only`
    /// limits which assets the pool may hold.
    AdditionalSlots {
        /// The slot type granted.
        slot: Slot,
        /// How many slots of that type.
        count: u8,
        /// The assets these slots can hold. `None` is any asset of the type.
        only: Option<CardFilter>,
    },
    /// **Treat one revealed chaos symbol as another**, for each token the
    /// source card's controller reveals: The Necronomicon 01009's *"Treat each
    /// `[elder_sign]` you reveal on a chaos token as a `[auto_fail]`."* A
    /// constant marker like [`Restrict`](Self::Restrict), under a
    /// [`Trigger::Constant`]: **inspected, not executed** — the skill test
    /// reads it off the performer's cards as the token comes out of the bag,
    /// and everything downstream of the reveal reads the symbol it counts as.
    /// Resolving it rejects.
    TreatRevealedToken {
        /// The symbol revealed.
        from: ChaosSymbol,
        /// The symbol it counts as.
        to: ChaosSymbol,
    },
    /// **An event goes to the bottom of its owner's deck instead of their
    /// discard pile**, after they play it or when it is discarded from play:
    /// Wendy's Amulet 01014's *"**Forced** - After you play an event or discard
    /// an event from play: Place it on the bottom of your deck instead of in
    /// your discard pile."* A constant marker like
    /// [`AdditionalSlots`](Self::AdditionalSlots), under a
    /// [`Trigger::Constant`]: **inspected, not executed** — the engine reads it
    /// off the owner's cards where it places the event, so it is a routing rule
    /// rather than an ability with a window of its own. Resolving it rejects.
    PlaceEventsOnBottomOfDeck,
    /// **Its controller may play the topmost card of their discard pile, if it
    /// is an event, as if it were in their hand**: Wendy's Amulet 01014's *"You
    /// may play the topmost event in your discard pile as if it were in your
    /// hand."* A play permission, under a [`Trigger::Constant`]: **inspected,
    /// not executed** — the play paths read it off the controller's cards when
    /// they list where a card may be played from. It reaches plays only, never
    /// a commit (<https://arkhamdb.com/card/01014>: *"'Play' and 'Commit' are
    /// two different game terms."*). Resolving it rejects.
    PlayTopmostEventInDiscardPile,
    /// Search a region of an investigator's deck for one card matching
    /// `filter`, move it to that investigator's hand, then shuffle the deck.
    /// Old Book of Lore 01031 (top 3, any card, chosen investigator) and
    /// Research Librarian 01032 (entire deck, a `Tome` asset, you).
    ///
    /// Rules Reference p.18 ("Search"): the searcher is *obligated to find* a
    /// card if one or more eligible options exist (no decline) — so 0 eligible
    /// ⇒ find nothing, 1 ⇒ auto-take, 2+ ⇒ the controller picks. An entire-deck
    /// search must shuffle on completion; top-N shuffles too (Old Book
    /// "shuffles the remaining cards into the deck"). Both "draws it" (Old
    /// Book) and "add to your hand" (Librarian) are modeled as a move to hand.
    /// Either way a weakness taken resolves as if drawn: `glossary/Weakness.md`,
    /// *"If a weakness enters an investigator's hand in a manner that did not
    /// involve drawing the card, that investigator must resolve the card
    /// (including any Revelation abilities) as if he or she had just drawn
    /// it"* (The Necronomicon 01009). Its Revelation resolves after the
    /// shuffle.
    SearchDeck {
        /// Whose deck is searched.
        target: InvestigatorTarget,
        /// Which region of the deck to look at.
        scope: SearchScope,
        /// Which cards are eligible. `None` matches every card.
        filter: Option<CardFilter>,
    },
    /// The currently-playing event attaches itself to its controller's current
    /// location (Barricade 01038): take the card off the frame driving its play
    /// and re-home that same card into the location's attachment zone, instead of
    /// letting it discard. One card — hand → location attachment → (on a later
    /// effect) discard; no duplicate spawned by code (cf.
    /// [`PutIntoThreatArea`](Self::PutIntoThreatArea), which spawns by code only
    /// because an *encounter* card has no instance at Revelation time).
    ///
    /// TODO(#373): generalize into a shared attach-to-location effect (a
    /// by-code form + an optional per-location limit) so Obscuring Fog 01168's
    /// bespoke `limit1-attach` native collapses onto it.
    AttachSelfToLocation,
    /// **Grant abilities to a card.** A constant effect, *inspected* by the
    /// grant sweep (`game_core::engine::abilities_in_effect`) rather than
    /// executed by the evaluator — resolving one rejects, as
    /// [`Restrict`](Self::Restrict) does.
    ///
    /// `glossary/Gains.md`: *"If a card gains a characteristic (such as an
    /// icon, a trait, a keyword, or ability text), the card functions as if it
    /// possesses the gained characteristic."* — and *"'Gained' characteristics
    /// are not considered to be 'printed' on the card."* So the recipient
    /// **has** the granted ability: it is the source an activation names, and
    /// it is the recipient's ability that resolves. It is simply not printed
    /// there, which is why the sweep merges it in one layer above the registry
    /// and why the engine's `AbilityAddress` names the *granter*.
    ///
    /// The two Core consumers, verbatim
    /// (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
    ///
    /// > \[Parlor 01115\] While Lita Chantler is not controlled by a player,
    /// > she gains: "\[action\]: **Parley.** Test \[intellect\] (4). If you
    /// > succeed, take control of Lita Chantler."
    ///
    /// > \[Lita Chantler 01117\] While you control Lita Chantler, she gains:
    /// > "Each investigator at your location gets +1 \[combat\]. …"
    ///
    /// # The condition is a field, not an `Effect::If` around it
    ///
    /// **`Effect::If { then: Grant }` is silently invisible to the sweep.** A
    /// `Trigger::Constant` effect is inspected, never executed, so it cannot
    /// borrow the evaluator's control flow: there is no resolution in flight
    /// and the "else" of *"this ability applies"* is silence. A constant effect
    /// that wants a condition carries it as data. The alternative — teaching a
    /// sweep to see through `Effect::If` — is the gap **#679** already tracks
    /// for [`Modify`](Self::Modify), and two sweeps with different traversal
    /// power would give two answers to *"does this constant effect apply"*.
    /// Each granting card's own test pins the bare shape.
    ///
    /// # Who "you" is
    ///
    /// The sweep evaluates `condition` against the **recipient's** controller,
    /// which is `None` for a card in play under nobody's control. A condition
    /// that needs a "you" does not hold against `None`; a board-global one
    /// ([`Condition::ControlStatus`]) is answered anyway.
    Grant {
        /// Which card receives the abilities.
        to: GrantTarget,
        /// When the grant applies. `None` grants unconditionally.
        condition: Option<Condition>,
        /// The granted abilities, in printed order. The engine's
        /// `AbilityAddress::Granted { sub }` indexes this vector, so its order
        /// is part of the addressing.
        abilities: Vec<Ability>,
    },
    /// **Take control of the in-play card printed with `code`**, moving that
    /// card's *same* instance into the resolving controller's play area.
    ///
    /// The Parlor 01115's Parley — *"If you succeed, take control of Lita
    /// Chantler."* — is the corpus's first consumer; ~129 cards print the
    /// verb, with "Jazz" Mulligan 02060, All In 02068 and Fold 02069 next in
    /// the near backlog.
    ///
    /// **The instance moves; it is not re-minted.** The ruling is explicit
    /// (<https://arkhamdb.com/card/01117>): *"When you 'take control' of a
    /// card, it enters your play area (not your hand)."* — so the card's
    /// accumulated damage and horror, its uses pool and its per-ability usage
    /// counters all travel with it.
    ///
    /// **Control is not ownership.** Taking control leaves the card's owner
    /// alone, which is the whole content of *"You take control of Lita only
    /// **temporarily**, until the end of the scenario. Taking control of her
    /// doesn't make her a part of your deck."* Where the card goes when it
    /// later leaves play is a question about its **owner**, and that is what
    /// makes Lita's removal derive rather than be special-cased.
    ///
    /// **Slot pressure applies.** `glossary/Slots.md`: *"If playing **or
    /// gaining control** of an asset would put an investigator above his or her
    /// slot limit for that type of asset, the investigator must choose and
    /// discard other assets under his or her control simultaneously with the
    /// new asset entering the slot."* — the same make-room prompt a play from
    /// hand raises.
    ///
    /// `TODO(#824)`: the corpus's other entry path is a **set-aside** card —
    /// Harold Walsted 02124's *"Choose an investigator to take control of the
    /// set-aside Harold Walsted"* — which both names a chosen controller and
    /// pulls from out of play. Out of scope: *"take control of 1 of the clues
    /// on …"* (03076a), a different verb on a different noun.
    TakeControl {
        /// Printed `ArkhamDB` code of the card to take control of.
        code: String,
    },
    /// **Grant an additional action this turn** (see **Standard action /
    /// additional action** in `GLOSSARY.md`): one more action the investigator
    /// may take this turn, on top of the three standard ones.
    /// `glossary/Action.md` (FAQ 1.10): *"Some card abilities grant
    /// investigators "additional actions.""*
    ///
    /// **Executed**, by an ability that resolves: "Skids" O'Toole 01003's
    /// *"spend 2 resources: You may take an additional action this turn."* The
    /// grant is pushed onto the controller and lasts until it is used or the
    /// turn ends (FAQ 1.31: *"Any unused actions are forfeited at the end of an
    /// investigator's turn"*). A card in play that grants one each turn is the
    /// constant marker
    /// [`AdditionalActionWhileInPlay`](Self::AdditionalActionWhileInPlay)
    /// instead.
    ///
    /// The engine never stores a count of additional actions. It works the list
    /// out each time it's needed, so a grant from a card in play is gone the
    /// moment the card leaves play.
    GainAdditionalAction {
        /// What the additional action may be used for. `None` is any action.
        restriction: Option<AdditionalActionRestriction>,
    },
}

/// What an additional action ([`Effect::GainAdditionalAction`],
/// [`Effect::AdditionalActionWhileInPlay`]) may be spent on — Daisy Walker
/// 01002's *"You may take an additional action during your turn, which can
/// only be used on \[\[Tome\]\] \[action\] abilities."*
///
/// **Inspected, not executed**: the engine tests the action being taken against
/// it each time it works out which additional actions can pay for that action.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdditionalActionRestriction {
    /// Only an `[action]` ability (an action cost of at least one) on a card in
    /// play whose printed metadata matches the filter. Playing a card, a basic
    /// action, and an ability on a location, enemy, act or agenda never
    /// qualify. Daisy Walker 01002 filters on the Tome trait.
    ActivatedAbilityOn(CardFilter),
}

/// Which card an [`Effect::Grant`] gives its abilities to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GrantTarget {
    /// The granting card itself — Lita Chantler 01117's *"While you control
    /// Lita Chantler, **she** gains …"*, printed on Lita.
    SelfCard,
    /// The in-play card printed with this code — the Parlor 01115's *"While
    /// Lita Chantler is not controlled by a player, **she** gains …"*, printed
    /// on the Parlor and naming a different card.
    ///
    /// A `CardCode` in spirit; a bare `String` because `card-dsl` is the bottom
    /// of the crate stack and the newtype lives in `game-core`.
    Card(String),
}

/// Which region of a deck an [`Effect::SearchDeck`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SearchScope {
    /// The top `n` cards (Old Book of Lore: 3). Fewer if the deck is shorter.
    Top(u8),
    /// The whole deck (Research Librarian). Must be shuffled on completion.
    EntireDeck,
}

/// A predicate on a printed card: an optional trait, an optional card type, and
/// whether weaknesses are excluded. Every narrowing set must hold; an empty
/// filter matches every card. Used by [`Effect::SearchDeck`] and by the
/// card-reading [`EventPattern`]s. `trait_` is owned (the
/// [`Effect`] enum is serde-serializable, so no borrowed `&'static str`).
///
/// Build one from a generated trait constant with the constructors, e.g.
/// `CardFilter::with_trait(TOME).and_kind(CardType::Asset)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardFilter {
    /// Required trait (e.g. `"Tome"`). `None` = any trait.
    pub trait_: Option<String>,
    /// Required card type (e.g. [`CardType::Asset`]).
    /// `None` = any type.
    pub kind: Option<CardType>,
    /// Whether weaknesses are excluded — Ward of Protection 01065's *"a
    /// non-weakness treachery card"*. `false` = weaknesses pass too.
    pub non_weakness: bool,
}

impl CardFilter {
    /// A filter for cards printing `trait_name`, of any type.
    #[must_use]
    pub fn with_trait(trait_name: &str) -> Self {
        Self {
            trait_: Some(trait_name.to_owned()),
            kind: None,
            non_weakness: false,
        }
    }

    /// A filter for cards of type `kind`, with any traits.
    #[must_use]
    pub fn of_kind(kind: CardType) -> Self {
        Self {
            trait_: None,
            kind: Some(kind),
            non_weakness: false,
        }
    }

    /// This filter, additionally excluding weaknesses
    /// ([`CardMetadata::is_weakness`]).
    #[must_use]
    pub fn and_non_weakness(self) -> Self {
        Self {
            non_weakness: true,
            ..self
        }
    }

    /// This filter, additionally requiring `trait_name`.
    #[must_use]
    pub fn and_trait(self, trait_name: &str) -> Self {
        Self {
            trait_: Some(trait_name.to_owned()),
            ..self
        }
    }

    /// This filter, additionally requiring card type `kind`.
    #[must_use]
    pub fn and_kind(self, kind: CardType) -> Self {
        Self {
            kind: Some(kind),
            ..self
        }
    }

    /// Whether the printed card `meta` passes the filter: it has the trait
    /// (via [`CardMetadata::has_trait`], so printed traits only), is of the
    /// type, and is not a weakness, for whichever of the three are set.
    #[must_use]
    pub fn matches(&self, meta: &CardMetadata) -> bool {
        self.trait_.as_deref().is_none_or(|t| meta.has_trait(t))
            && self.kind.is_none_or(|k| meta.card_type() == k)
            && !(self.non_weakness && meta.is_weakness())
    }
}

// ---- stats and modifier scopes --------------------------------

/// Which health track a heal or harm acts on — physical (`Damage`, on health)
/// or mental (`Horror`, on sanity). Shared by [`Effect::Heal`] and
/// [`Effect::Deal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HarmKind {
    /// Physical damage (reduces remaining health).
    Damage,
    /// Horror (reduces remaining sanity).
    Horror,
}

/// A statistic that an [`Effect::Modify`] can adjust.
///
/// Whose statistic it is falls out of the [`ModifierAudience`] the same
/// `Modify` carries: `MaxHealth` on an investigator audience is that
/// investigator's sanity-and-health capacity, while `MaxHealth` on an
/// enemy audience is the enemy's printed health (Towering Beasts 02256:
/// *"Attached enemy gets +1 fight and +1 health."*). Action points and
/// other "current" counters get added when cards in later cycles touch
/// them.
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
    /// An enemy's fight value (The Ritual Begins 01144: *"Each enemy
    /// gets +1 fight and +1 evade."*).
    Fight,
    /// An enemy's evade value (Cold Spring Glen 02244: *"Each enemy in
    /// Cold Spring Glen gets -1 evade."*).
    Evade,
    /// "Your skill value for that test" — whichever skill the in-flight
    /// test is against. Lucky! 01080: *"Get +2 to your skill value for
    /// that test."*; Ritual Candles 02029: *"You get +1 skill value for
    /// this test."*
    ///
    /// Meaningful only under [`ModifierScope::ThisSkillTest`], where the
    /// evaluator resolves it to the test's own skill as it records the
    /// row. A constant modifier has no test to name, so the sweep never
    /// matches it. Lucky! is its only corpus consumer, so it is a
    /// spec-approved exception to `standards.md`'s DSL-primitive threshold
    /// (#780).
    TestedSkill,
}

/// Who an [`Effect::Modify`] applies to, read off the printed text's
/// subject rather than off where the source card happens to sit.
///
/// A modifier is *found* by a sweep over every place a card can sit —
/// investigators' in-play cards, locations, enemies, their attachments,
/// and the current act and agenda — so the audience is what decides
/// whether the modifier a sweep found reaches the entity being asked
/// about. Without it a card on one investigator's board could not reach
/// another's, which most of the corpus's non-asset modifiers require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModifierAudience {
    /// "You get …" — the investigator who controls the source card.
    /// The overwhelmingly common case (Beat Cop 01018: *"You get +1
    /// \[combat\]."*) and the [`modify`] builder's default.
    Controller,
    /// "Each investigator at \<the source\>'s location gets …" — every
    /// investigator at the source's own location. Lita Chantler 01117
    /// (*"Each investigator at your location gets +1 \[combat\]."*),
    /// Whippoorwill 02090 (*"Each investigator at Whippoorwill's
    /// location gets -1 \[willpower\], -1 \[intellect\], -1 \[combat\], and
    /// -1 \[agility\]."*) and Whateley Ruins 02250 (*"Each investigator
    /// in Whateley Ruins gets -1 \[willpower\]."*) are all this one,
    /// from a controlled asset, an enemy and a location respectively.
    EachInvestigatorAtSourceLocation,
    /// "Each enemy at \<the source\>'s location gets …" — Cold Spring
    /// Glen 02244 (*"Each enemy in Cold Spring Glen gets -1 evade."*).
    EachEnemyAtSourceLocation,
    /// "Each enemy gets …" — every enemy in play, wherever the source
    /// sits. The Ritual Begins 01144 (*"Each enemy gets +1 fight and +1
    /// evade."*).
    EachEnemy,
    /// "Attached \<location/enemy\> gets …" — the entity the source card
    /// is attached to. Obscuring Fog 01168 (*"Attached location gets +2
    /// shroud."*) and Towering Beasts 02256 (*"Attached enemy gets +1
    /// fight and +1 health."*). Contributes nothing from a source that
    /// is not an attachment.
    AttachedCard,
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

/// A skill test's **determination**: the two-valued statement that the
/// test resolves a particular way whatever the numbers say.
///
/// `data/rules-reference/rules/glossary/Automatic_Failure_Success.md`:
///
/// > Some card or token abilities may cause a skill test to
/// > automatically fail or to automatically succeed.
/// >
/// > - If a skill test automatically fails, the investigator's total
/// >   skill value for that test is considered 0.
/// > - If a skill test automatically succeeds, the total difficulty of
/// >   that test is considered 0.
///
/// The two are **not** symmetric quantities of one axis: they substitute
/// different quantities, and automatic failure takes precedence over
/// automatic success. That precedence is resolved once for the whole
/// test, above the modified-value fold, because a fold evaluating either
/// quantity cannot see the other's rows — see ADR 0007.
///
/// Stored as a `RecordedModifier` row scoped to the test (over in
/// `game-core`, which this crate cannot name), so it inherits that
/// population's skill-test identity check, expiry sweep and abandonment
/// path. The `[auto_fail]`
/// chaos token writes one, and so does a card declaring one with
/// [`Effect::AutoResolve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Determination {
    /// The test automatically fails. Beats a simultaneous
    /// [`AutomaticSuccess`](Self::AutomaticSuccess).
    AutomaticFailure,
    /// The test automatically succeeds.
    AutomaticSuccess,
}

/// A restriction a card imposes: a constant one while the card is in play,
/// carried by a [`Trigger::Constant`] [`Effect::Restrict`], or a lasting one
/// a card-local native registers for a duration
/// ([`CannotAttack`](Restriction::CannotAttack)). The engine inspects these
/// at decision points rather than executing them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Restriction {
    /// The controller cannot play cards of this type (Dissonant Voices
    /// 01165 declares one per forbidden type — assets and events).
    CannotPlay(CardType),
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
    /// Enemies cannot move into the location this restriction's source is
    /// attached to, unless their printed card passes `exempt`.
    /// **Inspected, not executed** — the movers (`engine::dispatch::hunters`,
    /// the engaged drag-along, forced enemy moves) read it to decide whether
    /// an enemy may enter. It applies to the **compelled step alone**, leaving
    /// distances and shortest paths on the full connection graph — Hunter
    /// movement (#651) and agenda 01107's forced Ghoul move (#797) both read
    /// it that way, so a blocked step is a non-move rather than a detour.
    ///
    /// The exemption is the declaring card's own, not a rule: Barricade 01038
    /// prints *"Non-\[\[Elite\]\] enemies cannot move into attached location."*,
    /// so it supplies a "has the Elite trait" filter. The read site asks the
    /// card registry whether the moving enemy's code passes it.
    EnemyMovementBlocked {
        /// Enemies whose printed card passes this filter are not blocked.
        /// `None` blocks every enemy — there is no filter value that means
        /// "block nothing".
        exempt: Option<CardFilter>,
    },
    /// Investigators cannot move into the location this restriction's source
    /// is printed on or attached to (the Parlor 01115's unrevealed back:
    /// *"The entrance to the Parlor is blocked by a darkly glowing
    /// unfathomable barrier. You cannot move into the Parlor."*).
    /// **Inspected, not executed** — the Move action reads it, both when
    /// enumerating destinations and in its own validate-first, and it shares
    /// [`EnemyMovementBlocked`](Restriction::EnemyMovementBlocked)'s posture:
    /// the block applies to the **compelled step alone**, never to the
    /// connection graph.
    ///
    /// **The two sides are deliberately separate restrictions, not one**, and
    /// 01115's only `ArkhamDB` ruling is why: *"**Q:** Can enemies move into
    /// Parlor even when investigators are blocked by the barrier? **A:** Yes;
    /// in The Gathering scenario, enemies can move into The Parlor even when
    /// the investigators are blocked by the barrier."*
    /// (<https://arkhamdb.com/card/01115>). The Parlor blocks investigators
    /// and not enemies; Barricade 01038 blocks enemies and not investigators.
    /// The exemption filter belongs to the enemy side alone — the printed
    /// text names no investigator characteristic, so this variant carries
    /// none.
    InvestigatorMovementBlocked,
    /// Enemies cannot attack the investigator this restriction protects,
    /// unless their printed card passes `exempt` (On the Lam 01010: *"Until
    /// the end of the round, non-\[\[Elite\]\] enemies cannot attack you."*).
    /// **Inspected, not executed** — the three places an enemy attacks (an
    /// attack of opportunity, the Enemy phase's step 3.3, a Retaliate) leave
    /// a covered enemy out of the attack, so it neither attacks nor exhausts.
    ///
    /// **Lasting only.** The engine reads it from the lasting effects a
    /// card-local native registers through
    /// `NativeCx::restrict_until_end_of_round` (over in `game-core`), which names
    /// the protected investigator. A constant `CannotAttack` on a card in
    /// play has no reader, so declaring one is a silent no-op.
    ///
    /// The exemption is the declaring card's own, as with
    /// [`EnemyMovementBlocked`](Restriction::EnemyMovementBlocked): On the
    /// Lam supplies a "has the Elite trait" filter.
    CannotAttack {
        /// Enemies whose printed card passes this filter may still attack.
        /// `None` stops every enemy.
        exempt: Option<CardFilter>,
    },
    /// The card this restriction is printed on cannot leave play while
    /// `while_` holds (The Necronomicon 01009: *"It cannot leave play while it
    /// has 1 or more horror on it."*). `while_` is evaluated with the card as
    /// its source, so [`Quantity::HorrorOnSource`] reads the card's own horror.
    ///
    /// **Inspected, not executed** — the leave-play router in `game-core`
    /// reads it and refuses every exit while it holds, since
    /// `glossary/Cannot.md` makes no exception: *"The word "cannot" is
    /// absolute, and cannot be countermanded by other abilities."* A slot
    /// make-room never offers the card. Elimination is the one override. The
    /// rule is its step 0, which ends the game for the eliminated investigator
    /// and removes their weaknesses from the game. The code bypasses the check
    /// in steps 1 and 4, whose drains remove what the investigator controls and
    /// discard what is left in their threat area.
    CannotLeavePlay {
        /// While this holds, the card stays in play.
        while_: Condition,
    },
}

/// One action kind an [`Restriction::ExtraActionCost`] can surcharge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
/// (Investigate, Fight, Evade, plus a generic plain skill test)
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
    /// Any other skill test: treachery effects, agenda effects, or a plain skill test
    /// invoked directly (the synthetic `TestSession::test_skill` test entry point). Cards qualifying their bonus with one of the
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
    /// The candidate filter (an [`EnemyScope`] or a [`LocationSet`]).
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

/// An enemy-choice filter: a conjunction of constraints a candidate enemy must
/// meet. A card that narrows the choice further (Stray Cat 01076's trait
/// exclusion, [`exempt`](Self::exempt)) adds a field here rather than an arm,
/// so the constraints compose — *"an exhausted enemy at your location"* is one
/// scope with two fields set. A new field is optional, so [`EnemyScope::at`]
/// keeps reading as "no further constraint", touching neither [`LocationSet`]
/// nor location-picks.
///
/// Enemy choices only: an investigator choice is scoped by a bare
/// [`LocationSet`] (see [`InvestigatorTarget::Chosen`]), since an investigator
/// has no exhausted state for a filter to read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnemyScope {
    /// The enemy's location must be in this [`LocationSet`].
    pub at: LocationSet,
    /// `Some(true)` admits only exhausted enemies, `Some(false)` only ready
    /// ones, `None` either (Sneak Attack 01052, *"an exhausted enemy at your
    /// location"*).
    pub exhausted: Option<bool>,
    /// Enemies whose printed card passes this filter are **not** candidates
    /// (Stray Cat 01076's *"a non-\[\[Elite\]\] enemy"*); `None` excludes
    /// none. The same shape as
    /// [`CannotAttack`](Restriction::CannotAttack)'s `exempt`, matched
    /// against the enemy's card metadata, so reading it needs the card
    /// registry.
    pub exempt: Option<CardFilter>,
}

impl EnemyScope {
    /// An enemy at `set`, with no further constraint.
    #[must_use]
    pub const fn at(set: LocationSet) -> Self {
        EnemyScope {
            at: set,
            exhausted: None,
            exempt: None,
        }
    }

    /// This scope, additionally excluding enemies whose printed card passes
    /// `exempt`.
    #[must_use]
    pub fn except(self, exempt: CardFilter) -> Self {
        EnemyScope {
            exempt: Some(exempt),
            ..self
        }
    }
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
    /// The chooser picks one investigator at a location in the [`Choose`]'s
    /// [`LocationSet`]. Bound by the evaluator's `ground_chosen_targets` before
    /// the effect's handler runs.
    Chosen(Choose<LocationSet>),
    /// The investigator an enclosing [`Effect::ForEach`] is currently applying
    /// its body to — *"each investigator … discards"*. Bound per iteration by
    /// the evaluator's walking frame, separately from [`You`](Self::You), so a
    /// body can name both the card's controller and the iterated investigator.
    /// Resolving it outside a `ForEach` body over investigators is a rejection.
    Each,
}

impl InvestigatorTarget {
    /// "Choose an investigator" with no location constraint (any investigator
    /// in play). The successor to the bare `ChosenByController`.
    #[must_use]
    pub fn chosen_anywhere() -> Self {
        InvestigatorTarget::Chosen(Choose {
            scope: LocationSet::Anywhere,
        })
    }

    /// "Choose an investigator at your location."
    #[must_use]
    pub fn chosen_at_your_location() -> Self {
        InvestigatorTarget::Chosen(Choose {
            scope: LocationSet::Here,
        })
    }
}

/// Set-of-investigators target spec for [`Effect::ForEach`].
///
/// Either set holds only investigators still in the game (`Active`): one who
/// has resigned or been defeated is no longer an investigator the card can
/// reach. Both are walked in turn order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InvestigatorTargetSet {
    /// All investigators currently in the scenario, in turn order.
    All,
    /// All investigators at the controller's location.
    AtControllerLocation,
}

/// Set-of-enemies target spec for [`Effect::ForEach`], walked in enemy-id
/// order. The body names the current one as [`EnemyTarget::Each`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnemySet {
    /// Each enemy engaged with the controller — Elusive 01050's *"each enemy
    /// engaged with you"*.
    EngagedWithYou,
    /// Each enemy at the controller's location, engaged with anyone or no one
    /// — Cunning Distraction 01078's *"all enemies at your location"*. Empty
    /// when the controller is between locations.
    AtYourLocation,
}

/// What an [`Effect::ForEach`] fans out over: one arm per kind of entity, so
/// the walk is the same whatever it walks. The body reads the current one
/// through that kind's `Each` target ([`InvestigatorTarget::Each`],
/// [`EnemyTarget::Each`]).
///
/// [`for_each`] takes anything that converts into this, so a call site names
/// the set alone: `for_each(InvestigatorTargetSet::All, body)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ForEachTargets {
    /// A set of investigators.
    Investigators(InvestigatorTargetSet),
    /// A set of enemies.
    Enemies(EnemySet),
}

impl From<InvestigatorTargetSet> for ForEachTargets {
    fn from(set: InvestigatorTargetSet) -> Self {
        ForEachTargets::Investigators(set)
    }
}

impl From<EnemySet> for ForEachTargets {
    fn from(set: EnemySet) -> Self {
        ForEachTargets::Enemies(set)
    }
}

/// Single-location target spec.
///
/// **There is no "at" versus "in" distinction to encode.** Card text uses both
/// prepositions and the official FAQ says they are the same:
///
/// > Q: Is there any difference between "at a location" and "in a location?"
/// >
/// > A: No. Both terms have the same meaning and are used interchangeably.
///
/// (`data/official-faq/Frequently_Asked_Questions.md`.) So *"an enemy at your
/// location"* and *"each investigator in that location"* both ground through
/// the same variants here, and a card author must not reach for a new one on
/// the strength of the preposition alone. What *does* differ is whether a thing
/// is at a location at all — see the note on `Quantity::CluesAtControllerLocation`
/// in the evaluator, where clues on a card are not clues at the location.
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
    /// only reachable via a bare plain skill test from outside an
    /// Investigate path).
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

/// Single-enemy target spec. Another non-chosen form (`Engaged`, a specific
/// spawned enemy) lands with its first consumer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnemyTarget {
    /// The chooser picks one enemy from the [`Choose`]'s scope. Bound by
    /// `ground_chosen_targets` before the handler runs.
    Chosen(Choose<EnemyScope>),
    /// The enemy the enclosing [`Effect::ForEach`] over an [`EnemySet`] is
    /// currently running its body for. Valid only inside such a body.
    Each,
}

impl EnemyTarget {
    /// "Choose an enemy at your location."
    #[must_use]
    pub fn chosen_at_your_location() -> Self {
        EnemyTarget::Chosen(Choose {
            scope: EnemyScope::at(LocationSet::Here),
        })
    }
}

// ---- conditions -----------------------------------------------

/// A boolean predicate guarding an [`Effect::If`], an [`IntExpr::Cond`], a
/// [`grant`], or an ability's [`eligibility`](Ability::eligibility).
///
/// A minimal set, grown per consumer under the DSL-primitive threshold;
/// [`Native`](Self::Native) holds what it can't yet say.
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
    /// Compare a [`Quantity`] against `value` under `op`.
    /// Replaces the old `LocationHasClues` (now `Compare { CluesAtControllerLocation, Gt, 0 }`).
    Compare {
        quantity: Quantity,
        op: CmpOp,
        value: i8,
    },
    /// A card-local Rust predicate, resolved by tag through the host's
    /// `CardRegistry.native_condition_for`. The read-only mirror of
    /// [`Effect::Native`], and the **single** escape hatch for a read-only card
    /// predicate, whether it guards an effect or an ability's
    /// [`eligibility`](Ability::eligibility): a gate the declarative vocabulary
    /// above cannot express. The `cards` crate maps the tag to a
    /// `fn(&GameState, &EvalContext) -> bool`; the evaluator rejects loudly on an
    /// unknown tag or absent registry.
    ///
    /// Its consumers today:
    ///
    /// - **Machete 01020** — *"If the attacked enemy is the only enemy engaged
    ///   with you, this attack deals +1 damage."* A *conjunction* over the
    ///   *chosen target*, and neither half has a declarative form (no
    ///   [`Condition`] combinator, no target-referencing [`Condition`] or
    ///   [`Quantity`], and [`IntExpr::Cond`]'s branches are `i8` so conditions
    ///   cannot nest).
    /// - **They're Getting Out! 01107** — its reverse's act-3 branch, a plain
    ///   scenario-state read.
    /// - **Cover Up 01007** and **The Barrier 01109**, as eligibility — each
    ///   stands in for a change-state check its opaque [`Effect::Native`] hides.
    /// - **Lita Chantler 01117**, as eligibility — a trigger narrowing, and a
    ///   misfit there (see [`Ability::eligibility`]).
    /// - **"Skids" O'Toole 01003**, as eligibility — *"During your turn"* on a
    ///   `[fast]` ability, a play restriction no declarative condition names.
    ///
    /// `TODO(#609)`: this is an escape hatch, not a pattern. Lita's *"an
    /// investigator at your location successfully attacks a \[\[Monster\]\]
    /// enemy"* is the second compound, target-referencing predicate after
    /// Machete's, so it fired #609's promotion trigger: `Condition::All` plus a
    /// target-referencing variant, with Machete and Lita re-expressed through
    /// them, rather than more native tags of that shape. Issue #609 names the
    /// further in-corpus candidates (Oops! 02113, Esoteric Formula 02254,
    /// Springfield M1903 02226 — all Dunwich).
    Native { tag: String },
    /// **Whether the card printed with `code` is in the named control status.**
    ///
    /// `glossary/Ownership_and_Control.md` makes control a two-sided thing —
    /// *"A player controls the cards located in his or her out-of-play game
    /// areas"* against *"The scenario controls the cards in its out-of-play
    /// game areas"* — and cards print clauses on **both** sides of it. This
    /// variant asks which side a card is on rather than asserting one, because
    /// its two corpus consumers are complements on the same card:
    ///
    /// > \[Parlor 01115\] While Lita Chantler is **not** controlled by a
    /// > player, she gains: …
    ///
    /// > \[Lita Chantler 01117\] While **you control** Lita Chantler, she
    /// > gains: …
    ///
    /// 01117's is self-reducing: it is a self-grant, and the sweep binds "you"
    /// to the recipient's own controller, so *"you control Lita"* and *"Lita is
    /// controlled by a player"* are the same question. The two clauses
    /// therefore differ only in the [`ControlStatus`] they ask for, which is
    /// why the polarity is a **named field rather than a wrapping negation**.
    ///
    /// **Board-global: it reads no "you".** That is what lets a constant-effect
    /// sweep ask it on behalf of a card **nobody controls**, which is exactly
    /// the Parlor's case — a card-local [`Native`](Self::Native) predicate is
    /// `fn(&GameState, &EvalContext) -> bool`, and an `EvalContext` names a
    /// controller, so it could not be asked at all. See ADR 0014.
    ///
    /// "Jazz" Mulligan 02060 prints 01115's clause about himself, and Clover
    /// Club Lounge/Bar/Cardroom 02071-73 print 01117's, so the near backlog
    /// wants both statuses too.
    ///
    /// **`code` rather than an instance, and the corpus says that is enough.**
    /// The question is answered by a board scan for the code, so two copies of
    /// one card cannot be told apart — which would matter only to a card saying
    /// *"while you control **this**"* while able to have two copies in play.
    /// Every card in the snapshot that both prints a grant and gates it on
    /// control is quantity 1 (01115, 01117, 02060, 02068, 02069, 10051, 70040,
    /// 05038); the two multi-copy entries — Well-Funded 10051 and Able Bodied
    /// 05038 — gate on *other* cards' traits rather than on themselves, and
    /// their conditions already bind the recipient's own controller and source
    /// instance through the sweep. [`GrantTarget::SelfCard`] is instance-scoped
    /// regardless, so only the condition is code-keyed.
    ControlStatus {
        /// Printed `ArkhamDB` code of the card whose control is being asked
        /// about.
        code: String,
        /// The status the card must be in for the condition to hold.
        status: ControlStatus,
    },
}

/// Which side of `glossary/Ownership_and_Control.md`'s control split a card is
/// on, for [`Condition::ControlStatus`].
///
/// Two named states rather than a `bool`, so a card's printed polarity is
/// legible at the declaration site and at the read site alike: `status: false`
/// says nothing about *what* is false.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlStatus {
    /// In play and controlled by a player — Lita Chantler 01117's *"While
    /// **you control** Lita Chantler"*.
    ByAPlayer,
    /// Controlled by no player: in play under nobody's control (the Parlor
    /// 01115's Lita before a Parley), or not in play at all. The Parlor's
    /// *"While Lita Chantler is **not** controlled by a player"*.
    ///
    /// **Not the same as "controlled by the scenario"**, which is the narrower
    /// thing `Ownership_and_Control.md` names for the scenario's own out-of-play
    /// areas. No card prints that distinction yet; the day one does, it is a
    /// third status rather than a re-reading of this one.
    ByNoPlayer,
}

/// A non-negative count read off game state, usable as a value
/// ([`IntExpr::Count`]) or compared in a predicate ([`Condition::Compare`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quantity {
    /// Clues on the controller's current location.
    CluesAtControllerLocation,
    /// Horror on the card the evaluation is sourced to, wherever it sits; `0`
    /// with no source card. Agnes Baker 01004's `[elder_sign]` effect, *"+1 for
    /// each horror on Agnes Baker."*, is sourced to her investigator card, so it
    /// reads her own horror and not the horror on her assets. The Necronomicon
    /// 01009's *"while it has 1 or more horror on it"* reads the card's own.
    HorrorOnSource,
    /// Enemies engaged with the controller.
    EngagedEnemies,
    /// Failure margin of the resolving skill test (0 outside one).
    SkillTestFailedBy,
    /// Success margin of the resolving skill test: its modified skill value
    /// minus its difficulty, read off the test's ST.6 verdict, so every
    /// modifier added during the test counts. `0` outside a passed test's ST.7
    /// results. Switchblade 01044 and .41 Derringer 01047's *"If you succeed by
    /// 2 or more, this attack deals +1 damage."*
    SkillTestSucceededBy,
    /// Cards the controller controls that match the filter, in their play
    /// area and their threat area — Daisy Walker 01002's *"draw 1 card for
    /// each \[\[Tome\]\] you control"*. Their investigator card is not
    /// counted.
    ///
    /// Boxed so a `Quantity` stays pointer-sized: it sits inline in every
    /// [`Condition`] and [`IntExpr`], and through them in every [`Effect`].
    ControlledCards(Box<CardFilter>),
}

/// Comparison operator for [`Condition::Compare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// An integer computed at effect-evaluation time. Lets a numeric field
/// carry a condition-gated value without duplicating the surrounding
/// effect — ".38 Special" reads its combat modifier as
/// `IntExpr::cond(Condition::Compare { quantity: Quantity::CluesAtControllerLocation, op: CmpOp::Gt, value: 0 }, 3, 1)`
/// rather than an [`Effect::If`] wrapping two near-identical `fight(…)` nodes.
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
    /// A state-read count ([`Quantity`]).
    Count(Quantity),
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

impl From<i8> for IntExpr {
    fn from(n: i8) -> Self {
        IntExpr::Lit(n)
    }
}

impl From<u8> for IntExpr {
    fn from(n: u8) -> Self {
        IntExpr::Lit(i8::try_from(n).unwrap_or(i8::MAX))
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
        designator: None,
        costs: Vec::new(),
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
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
        designator: None,
        costs: Vec::new(),
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
    }
}

/// [`on_play`], for an event that prints a bold [`ActionDesignator`] —
/// Blinding Light 01066's *"**Evade.** …"*, Backstab 01051's *"**Fight.** …"*.
///
/// As with [`activated_as`], **the designator performs the action and `effect`
/// is the residual** the text prints beside it; the modification the action
/// itself takes rides the designator. Playing the event *is* that action — the
/// FAQ's Ursula Downs 04002 answer counts an investigate action *"performed
/// via the play action"* — see [`ActionDesignator`].
#[must_use]
pub fn on_play_as(designator: ActionDesignator, effect: Effect) -> Ability {
    Ability {
        designator: Some(designator),
        ..on_play(effect)
    }
}

/// Construct a [`Trigger::OnCommit`]-driven [`Ability`] wrapping the
/// given effect. Used by skill cards and other commit-trigger cards.
#[must_use]
pub fn on_commit(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::OnCommit,
        designator: None,
        costs: Vec::new(),
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
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
        designator: None,
        costs: Vec::new(),
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
    }
}

/// Construct a [`Trigger::OnEvent`] ability for the given pattern
/// and timing. Costs are empty; a reaction that prints one (Pickpocketing
/// 01046's *"exhaust Pickpocketing"*) chains [`Ability::with_costs`].
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
        designator: None,
        costs: Vec::new(),
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
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
        designator: None,
        costs: Vec::new(),
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
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
///
/// The ability prints **no bold action designator**; one that does is built by
/// [`activated_as`].
#[must_use]
pub fn activated(action_cost: u8, costs: Vec<Cost>, effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::Activated { action_cost },
        designator: None,
        costs,
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
    }
}

/// [`activated`], for an ability that prints a bold
/// [`ActionDesignator`] — Machete 01020's *"\[action\]: **Fight.**"*,
/// Flashlight 01087's *"\[action\] Spend 1 supply: **Investigate.**"*, the
/// Parlor 01115's *"\[action\] **Resign.**"*.
///
/// **The designator performs the action; `effect` is the residual** the printed
/// text prints *beside* the designated action, and is empty (`seq([])`) for
/// every card implemented today (#805). The modification the action itself takes
/// — Machete's `+1 [combat]`, Flashlight's `-2 [shroud]` — rides the designator,
/// so no effect can re-root a designated action into a different one; see
/// [`ActionDesignator`].
#[must_use]
pub fn activated_as(
    designator: ActionDesignator,
    action_cost: u8,
    costs: Vec<Cost>,
    effect: Effect,
) -> Ability {
    Ability {
        trigger: Trigger::Activated { action_cost },
        designator: Some(designator),
        costs,
        effect,
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
    }
}

/// Construct a modifier-only [`Trigger::ElderSign`]-driven [`Ability`]: its
/// [`TokenOutcome`] carries `modifier` and no effects (Roland Banks 01001).
/// [`elder_sign_with`] builds a sign that also has effects.
///
/// Called by investigator card impls in the `cards` crate and by tests in
/// `game-core` that build mock registries. (The `Ability` struct is
/// [`non_exhaustive`](https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute),
/// so a struct literal is only legal inside `card-dsl` itself.)
#[must_use]
pub fn elder_sign(modifier: IntExpr) -> Ability {
    elder_sign_with(TokenOutcome::modifier(modifier))
}

/// Construct a [`Trigger::ElderSign`]-driven [`Ability`] carrying the whole
/// `outcome`: its modifier and its `immediate` / `on_success` / `on_fail`
/// effects. The ability's own `effect` slot is the inert empty `Seq` for
/// every elder sign; a sign's effects belong in its outcome's arms, never in
/// that slot.
#[must_use]
pub fn elder_sign_with(outcome: TokenOutcome) -> Ability {
    Ability {
        trigger: Trigger::ElderSign { outcome },
        designator: None,
        costs: Vec::new(),
        effect: Effect::Seq(Vec::new()),
        usage_limit: None,
        eligibility: None,
        active_in: AbilityZone::InPlay,
    }
}

/// Build an [`Effect::GainResources`].
#[must_use]
pub fn gain_resources(target: InvestigatorTarget, amount: u8) -> Effect {
    Effect::GainResources { target, amount }
}

/// Build an unrestricted [`Effect::GainAdditionalAction`]: an additional action
/// this turn.
#[must_use]
pub fn gain_additional_action() -> Effect {
    Effect::GainAdditionalAction { restriction: None }
}

/// Build an unrestricted [`Effect::AdditionalActionWhileInPlay`], the constant
/// marker; wrap it in [`constant`].
#[must_use]
pub fn additional_action_while_in_play() -> Effect {
    Effect::AdditionalActionWhileInPlay { restriction: None }
}

/// Build an [`Effect::AdditionalActionWhileInPlay`] that only the actions
/// `restriction` admits can use; wrap it in [`constant`].
#[must_use]
pub fn restricted_additional_action_while_in_play(
    restriction: AdditionalActionRestriction,
) -> Effect {
    Effect::AdditionalActionWhileInPlay {
        restriction: Some(restriction),
    }
}

/// Build an [`Effect::TreatRevealedToken`]: each `from` its controller reveals
/// counts as `to`. The constant marker; wrap it in [`constant`].
#[must_use]
pub fn treat_revealed_token(from: ChaosSymbol, to: ChaosSymbol) -> Effect {
    Effect::TreatRevealedToken { from, to }
}

/// Build an [`Effect::DiscoverClue`].
#[must_use]
pub fn discover_clue(from: LocationTarget, count: u8) -> Effect {
    Effect::DiscoverClue { from, count }
}

/// Build an [`Effect::Deal`] dealing `amount` damage to `target`.
#[must_use]
pub fn deal_damage(target: InvestigatorTarget, amount: impl Into<IntExpr>) -> Effect {
    Effect::Deal {
        kind: HarmKind::Damage,
        target,
        amount: amount.into(),
    }
}

/// Build an [`Effect::Heal`].
#[must_use]
pub fn heal(kind: HarmKind, target: InvestigatorTarget, count: u8) -> Effect {
    Effect::Heal {
        kind,
        target,
        count,
    }
}

/// Build an [`Effect::Heal`] healing `count` damage from `target` (the
/// healing analogue of [`deal_damage`]).
#[must_use]
pub fn heal_damage(target: InvestigatorTarget, count: u8) -> Effect {
    Effect::Heal {
        kind: HarmKind::Damage,
        target,
        count,
    }
}

/// Build an [`Effect::Heal`] healing `count` horror from `target` (the
/// healing analogue of [`deal_horror`]).
#[must_use]
pub fn heal_horror(target: InvestigatorTarget, count: u8) -> Effect {
    Effect::Heal {
        kind: HarmKind::Horror,
        target,
        count,
    }
}

/// Build an [`Effect::Deal`] dealing `amount` horror to `target`.
#[must_use]
pub fn deal_horror(target: InvestigatorTarget, amount: impl Into<IntExpr>) -> Effect {
    Effect::Deal {
        kind: HarmKind::Horror,
        target,
        amount: amount.into(),
    }
}

/// Build an [`Effect::DealDamageToEnemy`].
#[must_use]
pub fn deal_damage_to_enemy(target: EnemyTarget, amount: u8) -> Effect {
    Effect::DealDamageToEnemy { target, amount }
}

/// Build an [`Effect::Disengage`].
#[must_use]
pub fn disengage(target: EnemyTarget) -> Effect {
    Effect::Disengage { target }
}

/// Build an [`Effect::AutoEvade`].
#[must_use]
pub fn auto_evade(target: EnemyTarget) -> Effect {
    Effect::AutoEvade { target }
}

/// Build an [`Effect::BoostAttackDamage`] adding `amount` to the
/// in-flight Fight test's bonus damage (Vicious Blow 01025).
#[must_use]
pub fn boost_attack_damage(amount: u8) -> Effect {
    Effect::BoostAttackDamage(amount)
}

/// Build an [`Effect::DiscoverAdditionalClues`] adding `amount` to the
/// in-flight Investigate test's discovery count (Deduction 01039).
#[must_use]
pub fn discover_additional_clues(amount: u8) -> Effect {
    Effect::DiscoverAdditionalClues(amount)
}

/// Build an [`Effect::DrawCards`] drawing `count` cards for `target`. A
/// literal count is typed (`1u8`), since an untyped integer matches both of
/// [`IntExpr`]'s `From` impls.
#[must_use]
pub fn draw_cards(target: InvestigatorTarget, count: impl Into<IntExpr>) -> Effect {
    Effect::DrawCards {
        target,
        count: count.into(),
    }
}

/// Build an [`Effect::SearchDeck`].
#[must_use]
pub fn search_deck(
    target: InvestigatorTarget,
    scope: SearchScope,
    filter: Option<CardFilter>,
) -> Effect {
    Effect::SearchDeck {
        target,
        scope,
        filter,
    }
}

/// Build an [`Effect::AttachSelfToLocation`].
#[must_use]
pub fn attach_self_to_location() -> Effect {
    Effect::AttachSelfToLocation
}

/// Build an [`Effect::Modify`] aimed at the source's controller — the
/// "You get …" case. Use [`modify_for`] for every other audience.
#[must_use]
pub fn modify(stat: Stat, delta: i8, scope: ModifierScope) -> Effect {
    modify_for(ModifierAudience::Controller, stat, delta, scope)
}

/// Build an [`Effect::Modify`] aimed at `audience`.
#[must_use]
pub fn modify_for(
    audience: ModifierAudience,
    stat: Stat,
    delta: i8,
    scope: ModifierScope,
) -> Effect {
    Effect::Modify {
        stat,
        delta,
        scope,
        audience,
    }
}

/// Build an [`Effect::AutoResolve`] latching `determination` onto the
/// in-flight skill test.
#[must_use]
pub fn auto_resolve(determination: Determination) -> Effect {
    Effect::AutoResolve { determination }
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

/// Build an [`Effect::ForEach`] over `targets` — any set that converts into a
/// [`ForEachTargets`], such as an [`InvestigatorTargetSet`] or an [`EnemySet`].
#[must_use]
pub fn for_each(targets: impl Into<ForEachTargets>, body: Effect) -> Effect {
    Effect::ForEach {
        targets: targets.into(),
        body: Box::new(body),
    }
}

/// Build an [`Effect::ChooseOne`] from `(label, effect)` pairs — the label
/// each branch is offered under (see [`ChoiceBranch`]).
///
/// Empty `choose_one([])` is meaningless — there's nothing to pick —
/// and the evaluator treats it as a programmer error / log corruption
/// rather than a silent no-op. The DSL doesn't validate emptiness at
/// construction time because card declarations are constants and any card
/// author writing `choose_one([])` is making a typo we want to catch in tests
/// rather than silently swallow.
#[must_use]
pub fn choose_one<S: Into<String>>(branches: impl IntoIterator<Item = (S, Effect)>) -> Effect {
    Effect::ChooseOne(
        branches
            .into_iter()
            .map(|(label, effect)| ChoiceBranch {
                label: label.into(),
                effect,
            })
            .collect(),
    )
}

/// Build an [`Effect::AdvanceCurrentAct`].
#[must_use]
pub fn advance_current_act() -> Effect {
    Effect::AdvanceCurrentAct
}

/// Build an [`Effect::ReachResolution`] for the printed `(→R#)` number.
#[must_use]
pub fn reach_resolution(n: u8) -> Effect {
    Effect::ReachResolution(n)
}

/// Build an [`Effect::PlaceDoomOnCurrentAgenda`] for a card that prints the
/// placement **alone** — Silver Twilight Acolyte 01102, Saracenic Script
/// 02240, Blood on the Altar 02195. The doom waits for the Mythos phase's
/// threshold check.
#[must_use]
pub fn place_doom_on_current_agenda(count: impl Into<IntExpr>) -> Effect {
    Effect::PlaceDoomOnCurrentAgenda {
        count: count.into(),
        may_advance: false,
    }
}

/// Build an [`Effect::PlaceDoomOnCurrentAgenda`] for a card that also prints
/// *"this effect can cause the current agenda to advance"* — Ancient Evils
/// 01166, Dark Memory 01013, Offer of Power 01178. The threshold is checked as
/// soon as the doom lands.
#[must_use]
pub fn place_doom_that_can_advance_the_agenda(count: impl Into<IntExpr>) -> Effect {
    Effect::PlaceDoomOnCurrentAgenda {
        count: count.into(),
        may_advance: true,
    }
}

/// Build an [`Effect::Native`] referencing a host-registered Rust effect
/// by `tag` (convention: `"<cardcode>:<name>"`).
#[must_use]
pub fn native(tag: impl Into<String>) -> Effect {
    Effect::Native { tag: tag.into() }
}

/// *"If you succeed by `n` or more"*: [`Quantity::SkillTestSucceededBy`] at
/// least `n`. Switchblade 01044 and .41 Derringer 01047 (*"If you succeed by 2
/// or more, this attack deals +1 damage."*), Opportunist 01053 (*"If you
/// succeed by 3 or more"*).
#[must_use]
pub fn succeeded_by(n: i8) -> Condition {
    Condition::Compare {
        quantity: Quantity::SkillTestSucceededBy,
        op: CmpOp::Ge,
        value: n,
    }
}

/// Build a [`Condition::Native`] referencing a host-registered Rust predicate
/// by `tag` (same `"<cardcode>:<name>"` convention as [`native`]).
#[must_use]
pub fn native_condition(tag: impl Into<String>) -> Condition {
    Condition::Native { tag: tag.into() }
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
        horror: 0,
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
        horror: 0,
    }
}

/// Build an [`Effect::PutIntoThreatArea`] seeding `horror` on the placed
/// instance (The Necronomicon 01009: "Put The Necronomicon into play in your
/// threat area, with 3 horror on it").
#[must_use]
pub fn put_into_threat_area_with_horror(code: impl Into<String>, horror: u8) -> Effect {
    Effect::PutIntoThreatArea {
        code: code.into(),
        clues: 0,
        horror,
    }
}

/// Build an [`ActionDesignator::Fight`] carrying the modification the card
/// prints on its attack — the combat modifier and the bonus damage beyond the
/// base 1 (.38 Special: `fight(IntExpr::cond(Condition::Compare { quantity: Quantity::CluesAtControllerLocation, op: CmpOp::Gt, value: 0 }, 3, 1), 1u8)`).
/// The fight itself is the designator's; this only says how it differs from a
/// basic one (#805). It tests `[combat]` and carries no reveal clause; chain
/// [`ActionDesignator::testing`] and [`ActionDesignator::with_reveal_clause`]
/// for a card that prints either (Shrivelling 01060).
#[must_use]
pub fn fight(
    combat_modifier: impl Into<IntExpr>,
    extra_damage: impl Into<IntExpr>,
) -> ActionDesignator {
    ActionDesignator::Fight {
        combat_modifier: combat_modifier.into(),
        extra_damage: extra_damage.into(),
        skill_override: None,
        reveal_clause: None,
    }
}

/// Build an [`ActionDesignator::Evade`]: a basic evasion, testing `[agility]`
/// with no reveal or success clause. Chain [`ActionDesignator::testing`],
/// [`ActionDesignator::with_reveal_clause`] and [`ActionDesignator::on_success`]
/// for what a card prints (Blinding Light 01066).
#[must_use]
pub fn evade() -> ActionDesignator {
    ActionDesignator::Evade {
        skill_override: None,
        reveal_clause: None,
        on_success: None,
    }
}

/// Build an [`ActionDesignator::Investigate`] applying `shroud_modifier` to the
/// controller's location difficulty for this investigation (Flashlight 01087:
/// `investigate(-2i8)`). A success discovers clues as the action does; chain
/// [`ActionDesignator::instead_of_discovery`] for a card that replaces that
/// (Burglary 01045).
#[must_use]
pub fn investigate(shroud_modifier: impl Into<IntExpr>) -> ActionDesignator {
    ActionDesignator::Investigate {
        shroud_modifier: shroud_modifier.into(),
        instead_of_discovery: None,
    }
}

/// Build an [`Effect::Restrict`] carrying a constant [`Restriction`].
#[must_use]
pub fn restrict(restriction: Restriction) -> Effect {
    Effect::Restrict(restriction)
}

/// Build an [`Effect::Grant`] — the constant *"X gains: '…'"* clause.
///
/// A **bare** `Grant` under [`Trigger::Constant`] is the only shape the grant
/// sweep sees; wrapping one in an [`Effect::If`] makes it silently inert. Pass
/// the gate as `condition` instead, which is what this builder's second
/// argument is for.
#[must_use]
pub fn grant(to: GrantTarget, condition: Option<Condition>, abilities: Vec<Ability>) -> Effect {
    Effect::Grant {
        to,
        condition,
        abilities,
    }
}

/// Build an [`Effect::TakeControl`] of the in-play card printed with `code`.
#[must_use]
pub fn take_control(code: impl Into<String>) -> Effect {
    Effect::TakeControl { code: code.into() }
}

/// Build a [`Condition::ControlStatus`] asking whether the card printed with
/// `code` is in `status`.
#[must_use]
pub fn control_status(code: impl Into<String>, status: ControlStatus) -> Condition {
    Condition::ControlStatus {
        code: code.into(),
        status,
    }
}

/// Build an [`Effect::SkillTest`] initiating a `skill` test against
/// `difficulty`. `on_success` runs after a passing draw, `on_fail` after a
/// failing one (with the margin in the evaluator context's `failed_by`);
/// either may be `None`. Most cards branch on exactly one side — failure
/// (the one-shot Revelation treacheries) or success (Frozen in Fear 01164).
#[must_use]
pub fn skill_test(
    skill: SkillKind,
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

// ---- tests ----------------------------------------------------

#[cfg(test)]
mod tests;
