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
//!   than the controller). Machete 01020 wants both at once and holds them in a
//!   [`Condition::Native`] predicate; `TODO(#609)` — the second card to want
//!   either is the trigger to promote them to declarative vocab instead of
//!   registering another native tag.
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

/// The **bold action designator** an activated ability prints (`glossary/Ability.md`,
/// "Action Designators"), verbatim:
///
/// > Some abilities have bold action designators (such as **Fight**, **Evade**,
/// > **Investigate**, or **Move**). Activating such an ability performs the
/// > designated action as described in the rules, but modified in the manner
/// > described by the ability.
///
/// It is **declared on the trigger, not inferred from the effect**, because the
/// rules quote the designator and not the effect the ability happens to have.
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
/// `TODO(#778)`: the designator rides [`Trigger::Activated`] because no
/// implemented card prints one on another trigger. Backstab 01051 — which #778
/// brings in with the rest of the level-0 Rogue pool — prints a bold
/// **Fight** on an *event* (`Trigger::OnPlay`) and the rules attach the
/// designator to the ability rather than to how it was reached — the FAQ
/// answers for Ursula Downs 04002 that *"Ursula's reaction allows you to take
/// any investigate action, including those performed via the activate action or
/// via the play action."* Promoting the field to [`Ability`] is mechanical when
/// Backstab lands; doing it now would be speculation.
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
        /// Bonus damage beyond the base 1 (.38 Special: +1).
        extra_damage: IntExpr,
    },
    /// **Evade** — performs an evade action. Ten corpus cards print it, none
    /// implemented, so the engine rejects a designated Evade rather than
    /// guessing at a payload (`TODO(#818)`). Eight are **events** whose bold
    /// word rides `Trigger::OnPlay` (Blinding Light 01066/01069, Cunning
    /// Distraction 01078, Bind Monster 02031, Bait and Switch 02034), which
    /// needs the designator promoted off [`Trigger::Activated`] first — the
    /// `TODO(#778)` above. The two that are already this shape are assets:
    /// Fire Extinguisher 02114's *"\[action\] Exile Fire Extinguisher:
    /// **Evade.** You get +3 \[agility\] for this test…"*, whose `+3` is the
    /// [`Fight`](Self::Fight) modification's twin, and Strange Solution 02264's
    /// *"\[action\] Spend 1 supply: **Evade.** Evade with a base \[agility\]
    /// skill of 6."*, whose **base-value replacement** (`CONTEXT.md`, "Base
    /// value") is a shape no designator payload carries yet. So the field is
    /// unfixed because two live cards disagree about what it should be, not
    /// for want of a sample.
    Evade,
    /// **Move** — performs a move action. **No corpus card prints it**, though
    /// 29 in the snapshot do (Sled Dog 08127's *"\[action\] Exhaust X Sled
    /// Dogs: **Move.** Move X times."*, Ring Library 11624's *"\[action\]:
    /// **Move.** Move to a revealed \[\[Passageway\]\] location."*), so its
    /// modification is a *destination or a repeat count* rather than a stat
    /// row. Rejected on the same `TODO(#818)` as [`Evade`](Self::Evade), for
    /// the stronger reason: nothing the build compiles prints one at all. Named
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
    /// follow-up, so on success it discovers a clue like the action does.
    Investigate {
        /// Shroud-difficulty modifier for this investigation
        /// (Flashlight: `-2`). Evaluated at every read while the
        /// investigation runs, like [`Fight`](Self::Fight)'s combat
        /// modifier.
        shroud_modifier: IntExpr,
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
            Self::Evade => Some(ActionClass::Evade),
            Self::Move => Some(ActionClass::Move),
            Self::Investigate { .. } | Self::Parley | Self::Resign => None,
        }
    }
}

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
        /// The **bold action designator** the card prints above the
        /// effect, if any — `None` for an ability that prints none.
        ///
        /// This is what the rules key off, so it is declared rather than
        /// inferred from the effect tree: see [`ActionDesignator`].
        designator: Option<ActionDesignator>,
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
    /// `modifier` is the printed skill-test modifier the elder-sign grants,
    /// as an [`IntExpr`] so board-state-dependent values (Roland Banks's
    /// "+1 for each clue on your location" → `Count(CluesAtControllerLocation)`)
    /// resolve when the token is revealed. The engine adds this to the test total through
    /// the existing `Modifier` path (`skill_test.rs`), keeping the
    /// `ElderSign` resolution label for observability.
    ///
    /// Config-on-trigger, like [`Activated`](Self::Activated) /
    /// [`OnSkillTestResolution`](Self::OnSkillTestResolution).
    ///
    /// **Scope (#118):** only pure-*modifier* elder-signs are handled. Signs
    /// that also run an effect (Daisy's per-Tome draw, Agnes's optional
    /// damage) or substitute/reveal another token are deferred — the first
    /// such card should build a full `SymbolOutcome` from the investigator
    /// card for uniformity with the scenario symbol path.
    ElderSign {
        /// The printed skill-test modifier the elder-sign grants.
        modifier: IntExpr,
    },
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
    ///
    /// Matched only by the forced dispatch path
    /// (`engine::dispatch::forced_triggers`), never by player reaction
    /// windows — `trigger_matches` returns `false` for it.
    PhaseStarted { phase: Phase },
    /// A game phase ended. Forced agenda/act effects keyed to a phase
    /// boundary listen here: agenda `01107` moves Ghouls at
    /// `PhaseEnded { phase: Enemy }`. (Its end-of-round doom keys off
    /// [`RoundEnded`](Self::RoundEnded), not `PhaseEnded { Upkeep }`.)
    ///
    /// Wired at all four phase-ends (#697); `Appendix_II_Timing_and_Gameplay.md`
    /// step 1.5 gives the end of a phase the same milestone sentence its step
    /// 1.1 gives the beginning.
    ///
    /// Matched only by the forced dispatch path
    /// (`engine::dispatch::forced_triggers`), never by player reaction
    /// windows — `trigger_matches` returns `false` for it.
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
    /// A skill test resolved with the given `outcome` (RR ST.6). The
    /// card-facing narrowing of the engine's ST.6→ST.7 timing point — the
    /// general form of which "after you successfully investigate" (Dr. Milan
    /// 01033 reaction, Obscuring Fog 01168 forced) is the `{ Success,
    /// Investigate }` case. `kind: None` matches any test type; `Some(k)`
    /// narrows to that type. Forced vs reaction is the `OnEvent { kind }`
    /// distinction (Obscuring Fog `Forced`, Dr. Milan `Reaction`), not a
    /// pattern distinction — both share this pattern. The engine binds *you* =
    /// the testing investigator; a forced ability on a location attachment is
    /// scanned via the investigated location (the forced collector reads it
    /// from the in-flight test frame). (Slice D #423; collapses the #212/#213
    /// forced/reaction pattern split for this timing point.)
    SkillTestResolved {
        /// Whether the listener fires on a passed or failed test.
        outcome: TestOutcome,
        /// Narrow to a test type, or `None` for any.
        kind: Option<SkillTestKind>,
    },
    /// An investigator discovers one or more clues — the **one** triggering
    /// condition for clue discovery, reaction-only in all three cells (#703; a
    /// *forced* ability on it is not collected in any cell, since the condition
    /// has no forced dispatch point yet). Which cell an ability lands in is its
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
    /// The engine binds *you* = the discovering investigator and, for a `when`
    /// ability, the would-be discovery's count (Cover Up's "that many"). That
    /// count is what would **actually** be discovered — capped at the clues
    /// present, per the **Discovery** entry in `CONTEXT.md` (#471).
    DiscoverClues,
    /// The game ended (a scenario resolution latched). Fired forced via
    /// `ForcedTriggerPoint::GameEnd` from `fire_scenario_resolution`,
    /// scanning every investigator's controlled card instances; binds
    /// controller = each instance's controller. First consumer: Cover Up
    /// 01007's "Forced - When the game ends, if there are any clues on
    /// Cover Up: You suffer 1 mental trauma." (C5a #236.)
    GameEnd,
    /// An enemy attack assigned damage to the asset this ability is printed on
    /// (the soaked ally). Bare — the engine binds *self* = an asset the
    /// assignment gives damage to, the way
    /// [`EnteredLocation`](Self::EnteredLocation) / [`EndOfTurn`](Self::EndOfTurn)
    /// bind theirs, and binds the attacking enemy into the `EvalContext`.
    ///
    /// Pairs with the engine's `DamageAssigned` condition — Rules Reference step
    /// 1 of dealing damage, before anything is on any card — narrowed by this
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
    /// Bare — the "at your location" spatial scoping lives in the
    /// reaction-window scan (which has board state), mirroring the damaged-asset
    /// filter for [`EnemyAttackDamagedSelf`]; the forced scan reads the
    /// **attacking enemy's** own card.
    ///
    /// A cancel in the `when` cell suppresses the rest of the sequence, so an
    /// `at`/`after` ability on a dodged attack does not fire (#714;
    /// `data/arkhamdb-faq/core/01023.md`).
    ///
    /// [`EnemyAttackDamagedSelf`]: Self::EnemyAttackDamagedSelf
    EnemyAttacks,
    /// The card this ability is printed on entered play (Research Librarian
    /// 01032: "`[reaction]` After Research Librarian enters play: …"). Bare and
    /// **self-referential** — the engine fires it only for the just-entered
    /// instance (the reaction-window scan filters to that instance), binding
    /// *you* = the controller. A general "after any card enters play" reaction
    /// is out of scope; the pattern is reaction-only ([`EventTiming::After`]).
    EnteredPlay,
    /// An investigator left the location this ability's source is attached to
    /// (Barricade 01038's "Forced — When an investigator leaves attached
    /// location"). Bare and forced-only: the engine binds the leaving
    /// investigator (controller) and scans the *left* location's attachment
    /// zone. Matched only by the forced dispatch path
    /// (`ForcedTriggerPoint::LeftLocation`), never a reaction window.
    LeftLocation,
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
    /// occurs.'"* — because no corpus card puts both on one condition.
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
    /// Discard the source asset *in play* to pay for its own ability
    /// (Beat Cop 01018, Knife 01086). Distinct from
    /// [`Effect::DiscardSelf`], which removes a treachery from a threat
    /// area / location. Must be the only source-referencing cost on an
    /// ability (it removes the source); paid last.
    DiscardSelf,
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
    /// Payment costs (besides action cost). Empty for abilities with no
    /// non-action payment. Required on the wire (#453).
    pub costs: Vec<Cost>,
    pub effect: Effect,
    /// "Limit X per \[period\]" cap. `None` for abilities with no
    /// printed cap. Stays implicitly optional (#453 per-field reassessment):
    /// serde treats a missing `Option` field as `None`, the genuine
    /// absent-by-design case.
    pub usage_limit: Option<UsageLimit>,
    /// Native eligibility-predicate tag (resolved via the registry's
    /// `native_eligibility_for`). When set, the reaction/fast scan suppresses
    /// this ability unless the predicate holds (RR p.2: an ability can't
    /// initiate if its effect won't change game state). `None` for the common
    /// no-gate case; stays implicitly optional on the wire (#453 per-field
    /// carve-out, like `usage_limit`).
    pub eligibility: Option<String>,
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

    /// Attach a native eligibility-predicate tag (see [`Self::eligibility`]).
    /// Builder-style sugar, chainable off the `on_event(...)` constructors.
    #[must_use]
    pub fn with_eligibility(mut self, tag: impl Into<String>) -> Self {
        self.eligibility = Some(tag.into());
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
    /// the body can refer to it.
    ForEach {
        targets: InvestigatorTargetSet,
        body: Box<Effect>,
    },
    /// Present alternatives to the controller. Resolves to the chosen
    /// branch's effect. Requires an `AwaitingInput` round-trip; the
    /// evaluator stub for this lands in Phase 3 alongside skill tests.
    ChooseOne(Vec<Effect>),
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
    /// Discard the firing card instance (the evaluator context's
    /// `source`). Locates the instance in a threat area or location
    /// attachment, removes it, and discards it to the encounter discard.
    /// Used by persistent treacheries' `Forced` self-discard abilities
    /// (Frozen in Fear 01164, Dissonant Voices 01165, Obscuring Fog
    /// 01168). Rejects if there is no source or the instance isn't found.
    DiscardSelf,
    /// Cancel the current cancellable game impact — the subject of the
    /// Before-timing window this effect resolves inside. Sets the engine's
    /// `pending_cancellation` signal, which the emit site honors after the
    /// window closes, skipping the prevented impact (an enemy attack's
    /// damage/horror — Dodge 01023; or a clue discovery — Cover Up 01007).
    /// RR p.6: the cancelled thing is "still regarded as initiated", only
    /// its effects are prevented. (Axis D #336.)
    ///
    /// A cancelled triggering condition takes **the rest of its sequence** with
    /// it: the `at` and `after` cells do not run, and neither does the rest of
    /// the `when` cell (#714). Dodge 01023's ruling is the citation, quoted at
    /// the engine's read site (`coordinator::prevented_in_the_when_cell`), which
    /// applies it to every coordinator-owned condition.
    ///
    /// Cancel is the degenerate replacement ("replace with nothing"): a card
    /// that replaces with its own effect runs that effect then `Cancel`
    /// (Cover Up = `Seq[discard-from-self, Cancel]`). Both suppress the rest of
    /// the sequence, so the engine models them as one signal.
    /// TODO(#366): a true replace-with-a-different-impact effect — the one
    /// replacement whose condition still resolves, and whose later cells
    /// therefore presumably still run.
    Cancel,
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
    /// the **Discovery** entry in `CONTEXT.md` and Deduction's module doc for
    /// the FAQ that settles it.
    DiscoverAdditionalClues(u8),
    /// A constant restriction the source card imposes while in play
    /// (under [`Trigger::Constant`]). **Inspected, not executed** — the
    /// engine reads it at the relevant decision point (`play_is_prohibited`
    /// for `CannotPlay`, `pending_action_surcharge` for `ExtraActionCost`);
    /// resolving it as an effect is a misuse and rejects.
    Restrict(Restriction),
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
    /// Book) and "add to your hand" (Librarian) are modeled as a move to hand —
    /// the only rules difference (on-draw triggers) has no Core consumer.
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
    /// ([`Condition::CardControlledByAPlayer`], and a
    /// [`Condition::Not`] of one) is answered anyway.
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

/// Eligibility predicate for an [`Effect::SearchDeck`]. Both fields, when
/// `Some`, must hold (trait AND type). `trait_` is owned (the [`Effect`] enum
/// is serde-serializable, so no borrowed `&'static str`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CardFilter {
    /// Required trait (e.g. `"Tome"`). `None` = any trait.
    pub trait_: Option<String>,
    /// Required card type (e.g. [`CardType::Asset`](crate::card_data::CardType::Asset)).
    /// `None` = any type.
    pub kind: Option<crate::card_data::CardType>,
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
    /// Non-Elite enemies cannot move into the location this restriction's
    /// source is attached to (Barricade 01038). **Inspected, not executed** —
    /// the movers (`engine::dispatch::hunters`) read it to decide whether a
    /// non-Elite enemy may enter. It applies to the **compelled step alone**,
    /// leaving distances and shortest paths on the full connection graph —
    /// Hunter movement (#651) and agenda 01107's forced Ghoul move (#797)
    /// both read it that way, so a blocked step is a non-move rather than a
    /// detour. The Elite exemption
    /// (RR: most movement-blockers exempt Elite) is applied at the read site,
    /// which has the moving enemy's traits.
    EnemyMovementBlocked,
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
    /// The **Elite exemption** belongs to the enemy side alone — there is no
    /// investigator analogue of it, so this variant has no read-site
    /// carve-out.
    InvestigatorMovementBlocked,
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
    /// invoked directly (the synthetic `perform_skill_test` test entry point). Cards qualifying their bonus with one of the
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

    /// "Choose an investigator at your location."
    #[must_use]
    pub fn chosen_at_your_location() -> Self {
        InvestigatorTarget::Chosen(Choose {
            scope: EntityScope::At(LocationSet::Here),
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

/// Single-enemy target spec. One variant today (`Chosen`); a non-chosen form
/// (`Engaged`, a specific spawned enemy) lands with its first consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EnemyTarget {
    /// The chooser picks one enemy from the [`Choose`]'s scope. Bound by
    /// `ground_chosen_targets` before the handler runs.
    Chosen(Choose<EntityScope>),
}

impl EnemyTarget {
    /// "Choose an enemy at your location."
    #[must_use]
    pub fn chosen_at_your_location() -> Self {
        EnemyTarget::Chosen(Choose {
            scope: EntityScope::At(LocationSet::Here),
        })
    }
}

// ---- conditions -----------------------------------------------

/// A boolean predicate guarding an [`Effect::If`].
///
/// Phase-2 minimal set; later phases will add things like
/// `Compare { CluesAtControllerLocation, Gt, 0 }`, `AnyEnemyEngaged`, comparisons against
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
    /// Compare a [`Quantity`] against `value` under `op`.
    /// Replaces the old `LocationHasClues` (now `Compare { CluesAtControllerLocation, Gt, 0 }`).
    Compare {
        quantity: Quantity,
        op: CmpOp,
        value: i8,
    },
    /// A card-local Rust predicate, resolved by tag through the host's
    /// `CardRegistry.native_condition_for`. The read-only mirror of
    /// [`Effect::Native`]: the escape hatch for a gate that the declarative
    /// vocabulary above cannot express and that only one card wants. The
    /// `cards` crate maps the tag to a `fn(&GameState, &EvalContext) -> bool`;
    /// the evaluator rejects loudly on an unknown tag or absent registry.
    ///
    /// Machete 01020's "if the attacked enemy is the only enemy engaged with
    /// you" is the sole consumer: it is a *conjunction* over the *chosen
    /// target*, and neither half has a declarative form (no [`Condition`]
    /// combinator, no target-referencing [`Condition`] or [`Quantity`], and
    /// [`IntExpr::Cond`]'s branches are `i8` so conditions cannot nest).
    ///
    /// `TODO(#609)`: this is an escape hatch, not a pattern. The **second**
    /// card that wants a compound or target-referencing condition should
    /// promote both halves to declarative vocab (`Condition::All` plus a
    /// target-referencing variant) and re-express Machete through it, rather
    /// than register a second native tag. Issue #609 names the in-corpus
    /// candidates that will fire it (Oops! 02113, Esoteric Formula 02254,
    /// Springfield M1903 02226 — all Dunwich).
    Native { tag: String },
    /// Negation: holds exactly when the inner condition does not.
    ///
    /// The first combinator in the vocabulary, and it arrives with the first
    /// card that prints a negated clause — the Parlor 01115's *"While Lita
    /// Chantler is **not** controlled by a player"*. Kept as a wrapper rather
    /// than folded into
    /// [`CardControlledByAPlayer`](Self::CardControlledByAPlayer) as a `bool`
    /// so the negation composes with every later variant instead of once with
    /// this one.
    ///
    /// **A negation is only as board-global as what it wraps** — see
    /// [`CardControlledByAPlayer`](Self::CardControlledByAPlayer) for what that
    /// buys a constant-effect sweep.
    Not(Box<Condition>),
    /// Whether the card printed with `code` is in play **and controlled by a
    /// player** — `glossary/Ownership_and_Control.md`: *"A player controls the
    /// cards located in his or her out-of-play game areas"*, against *"The
    /// scenario controls the cards in its out-of-play game areas"*. A card in
    /// play at a location under nobody's control is not controlled by a player,
    /// and neither is a card that is not in play at all.
    ///
    /// **Board-global: it reads no "you".** That is the whole reason it is a
    /// declarative variant rather than a card-local
    /// [`Native`](Self::Native) tag, which the one-card threshold in
    /// `docs/agents/standards.md` would otherwise call for. A native predicate
    /// is `fn(&GameState, &EvalContext) -> bool` and an `EvalContext` names a
    /// controller, so a native cannot be asked anything at all on behalf of an
    /// **uncontrolled** card — which is exactly the case the Parlor 01115's
    /// grant exists to serve: *"While Lita Chantler is not controlled by a
    /// player, she gains …"* is evaluated by the grant sweep with Lita as the
    /// recipient and no controller to bind. See ADR 0014.
    ///
    /// "Jazz" Mulligan 02060 prints the same clause about himself and is the
    /// second consumer waiting in the near backlog.
    CardControlledByAPlayer {
        /// Printed `ArkhamDB` code of the card whose control is being asked
        /// about.
        code: String,
    },
}

/// A non-negative count read off game state, usable as a value
/// ([`IntExpr::Count`]) or compared in a predicate ([`Condition::Compare`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quantity {
    /// Clues on the controller's current location.
    CluesAtControllerLocation,
    /// Enemies engaged with the controller.
    EngagedEnemies,
    /// Failure margin of the resolving skill test (0 outside one).
    SkillTestFailedBy,
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
        costs: Vec::new(),
        effect,
        usage_limit: None,
        eligibility: None,
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
        eligibility: None,
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
        eligibility: None,
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
        eligibility: None,
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
        eligibility: None,
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
        eligibility: None,
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
        trigger: Trigger::Activated {
            action_cost,
            designator: None,
        },
        costs,
        effect,
        usage_limit: None,
        eligibility: None,
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
        trigger: Trigger::Activated {
            action_cost,
            designator: Some(designator),
        },
        costs,
        effect,
        usage_limit: None,
        eligibility: None,
    }
}

/// Construct a [`Trigger::ElderSign`]-driven [`Ability`] with the given
/// modifier expression. The effect is empty (`Effect::Seq(vec![])`) because
/// pure-modifier elder-signs have no additional on-resolution effect; for
/// signs that run an effect (Daisy / Agnes) add a `Seq` with the desired
/// sub-effects.
///
/// Called by investigator card impls in the `cards` crate and by tests in
/// `game-core` that build mock registries. (The `Ability` struct is
/// [`non_exhaustive`](https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute),
/// so a struct literal is only legal inside `card-dsl` itself.)
#[must_use]
pub fn elder_sign(modifier: IntExpr) -> Ability {
    Ability {
        trigger: Trigger::ElderSign { modifier },
        costs: Vec::new(),
        effect: Effect::Seq(Vec::new()),
        usage_limit: None,
        eligibility: None,
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

/// Build an [`Effect::DrawCards`] drawing `count` cards for `target`.
#[must_use]
pub fn draw_cards(target: InvestigatorTarget, count: u8) -> Effect {
    Effect::DrawCards { target, count }
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

/// Build an [`ActionDesignator::Fight`] carrying the modification the card
/// prints on its attack — the combat modifier and the bonus damage beyond the
/// base 1 (.38 Special: `fight(IntExpr::cond(Condition::Compare { quantity: Quantity::CluesAtControllerLocation, op: CmpOp::Gt, value: 0 }, 3, 1), 1u8)`).
/// The fight itself is the designator's; this only says how it differs from a
/// basic one (#805).
#[must_use]
pub fn fight(
    combat_modifier: impl Into<IntExpr>,
    extra_damage: impl Into<IntExpr>,
) -> ActionDesignator {
    ActionDesignator::Fight {
        combat_modifier: combat_modifier.into(),
        extra_damage: extra_damage.into(),
    }
}

/// Build an [`ActionDesignator::Investigate`] applying `shroud_modifier` to the
/// controller's location difficulty for this investigation (Flashlight 01087:
/// `investigate(-2i8)`).
#[must_use]
pub fn investigate(shroud_modifier: impl Into<IntExpr>) -> ActionDesignator {
    ActionDesignator::Investigate {
        shroud_modifier: shroud_modifier.into(),
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

/// Build a [`Condition::Not`] — the negation of `condition`.
#[must_use]
pub fn not(condition: Condition) -> Condition {
    Condition::Not(Box::new(condition))
}

/// Build a [`Condition::CardControlledByAPlayer`] for the card printed with
/// `code`.
#[must_use]
pub fn card_controlled_by_a_player(code: impl Into<String>) -> Condition {
    Condition::CardControlledByAPlayer { code: code.into() }
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

// ---- tests ----------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The three designators `ActionClass` names map onto it; the three it
    /// doesn't name map to `None` (#754).
    #[test]
    fn a_designator_maps_onto_the_action_class_it_performs() {
        assert_eq!(fight(0u8, 0u8).action_class(), Some(ActionClass::Fight));
        assert_eq!(
            ActionDesignator::Evade.action_class(),
            Some(ActionClass::Evade)
        );
        assert_eq!(
            ActionDesignator::Move.action_class(),
            Some(ActionClass::Move)
        );
        for d in [
            investigate(0u8),
            ActionDesignator::Parley,
            ActionDesignator::Resign,
        ] {
            assert_eq!(d.action_class(), None, "{d:?} names no ActionClass");
        }
    }

    #[test]
    fn with_eligibility_sets_the_tag_and_default_is_none() {
        let bare = reaction_on_event(EventPattern::RoundEnded, EventTiming::When, Effect::Cancel);
        assert_eq!(bare.eligibility, None);
        let gated = reaction_on_event(EventPattern::RoundEnded, EventTiming::When, Effect::Cancel)
            .with_eligibility("01109:can_advance");
        assert_eq!(gated.eligibility.as_deref(), Some("01109:can_advance"));
    }

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
                audience: ModifierAudience::Controller,
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
                audience: ModifierAudience::Controller,
            }
        ));
        assert!(matches!(
            abilities[1].effect,
            Effect::Modify {
                stat: Stat::MaxHealth,
                delta: 1,
                scope: ModifierScope::WhileInPlay,
                audience: ModifierAudience::Controller,
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

    /// `Condition::Native` (#592) round-trips, and composes inside an
    /// [`IntExpr::Cond`] — the position Machete 01020 uses it from.
    #[test]
    fn native_condition_round_trips_through_serde_json() {
        let expr = IntExpr::cond(native_condition("01020:sole_engaged_target"), 1, 0);
        assert!(matches!(
            &expr,
            IntExpr::Cond {
                when: Condition::Native { tag },
                then: 1,
                otherwise: 0,
            } if tag == "01020:sole_engaged_target"
        ));
        let json = serde_json::to_string(&expr).expect("serialize");
        let recovered: IntExpr = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(expr, recovered);
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

    /// `Effect::DiscoverAdditionalClues` (Deduction 01039's "1 additional
    /// clue") round-trips through serde, and the builder constructs the
    /// variant.
    #[test]
    fn discover_additional_clues_round_trips_through_serde_json() {
        let effect = discover_additional_clues(1);
        assert_eq!(effect, Effect::DiscoverAdditionalClues(1));
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

    #[test]
    fn discard_self_cost_serde_round_trips() {
        let c = Cost::DiscardSelf;
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Cost>(&json).unwrap(), c);
    }

    #[test]
    fn heal_serde_round_trips() {
        let e = heal(
            HarmKind::Horror,
            InvestigatorTarget::chosen_at_your_location(),
            1,
        );
        assert_eq!(
            e,
            Effect::Heal {
                kind: HarmKind::Horror,
                target: InvestigatorTarget::chosen_at_your_location(),
                count: 1,
            }
        );
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Effect>(&json).unwrap(), e);
    }

    #[test]
    fn deal_builders_produce_the_kinded_effect_and_round_trip() {
        let dmg = deal_damage(InvestigatorTarget::You, 2u8);
        let hor = deal_horror(InvestigatorTarget::You, 3u8);
        assert_eq!(
            dmg,
            Effect::Deal {
                kind: HarmKind::Damage,
                target: InvestigatorTarget::You,
                amount: IntExpr::Lit(2),
            }
        );
        assert_eq!(
            hor,
            Effect::Deal {
                kind: HarmKind::Horror,
                target: InvestigatorTarget::You,
                amount: IntExpr::Lit(3),
            }
        );
        for e in [dmg, hor] {
            let json = serde_json::to_string(&e).unwrap();
            assert_eq!(serde_json::from_str::<Effect>(&json).unwrap(), e);
        }
    }

    #[test]
    fn deal_damage_to_enemy_serde_round_trips() {
        let e = deal_damage_to_enemy(EnemyTarget::chosen_at_your_location(), 1);
        assert_eq!(
            e,
            Effect::DealDamageToEnemy {
                target: EnemyTarget::Chosen(Choose {
                    scope: EntityScope::At(LocationSet::Here)
                }),
                amount: 1,
            }
        );
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Effect>(&json).unwrap(), e);
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

    #[test]
    fn search_deck_builder_and_serde_round_trip() {
        let e = search_deck(
            InvestigatorTarget::chosen_at_your_location(),
            SearchScope::Top(3),
            None,
        );
        assert!(matches!(
            e,
            Effect::SearchDeck {
                target: InvestigatorTarget::Chosen(_),
                scope: SearchScope::Top(3),
                filter: None,
            }
        ));
        let json = serde_json::to_string(&e).expect("serialize");
        let back: Effect = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);

        let filtered = search_deck(
            InvestigatorTarget::You,
            SearchScope::EntireDeck,
            Some(CardFilter {
                trait_: Some("Tome".into()),
                kind: Some(crate::card_data::CardType::Asset),
            }),
        );
        let json = serde_json::to_string(&filtered).expect("serialize");
        assert_eq!(
            filtered,
            serde_json::from_str::<Effect>(&json).expect("deserialize")
        );
    }

    #[test]
    fn barricade_dsl_variants_round_trip() {
        use crate::dsl::{attach_self_to_location, restrict, Restriction};
        let attach = attach_self_to_location();
        assert_eq!(attach, Effect::AttachSelfToLocation);
        let block = restrict(Restriction::EnemyMovementBlocked);
        for e in [attach, block] {
            let json = serde_json::to_string(&e).expect("ser");
            assert_eq!(e, serde_json::from_str::<Effect>(&json).expect("de"));
        }
        let pat = EventPattern::LeftLocation;
        let json = serde_json::to_string(&pat).expect("ser");
        assert_eq!(
            pat,
            serde_json::from_str::<EventPattern>(&json).expect("de")
        );
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
        let when_controller = Trigger::OnEvent {
            pattern: EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            timing: EventTiming::When,
            kind: TriggerKind::Reaction,
        };
        assert_ne!(after_any, Trigger::Constant);
        assert_ne!(after_any, Trigger::OnPlay);
        assert_ne!(after_any, Trigger::OnCommit);
        assert_ne!(after_any, after_controller);
        assert_ne!(after_controller, when_controller);
    }

    /// An `OnEvent`-triggered ability round-trips through `serde_json`
    /// — struct-variant × serde derive can surprise; pin the wire
    /// shape now so #52's persistence doesn't re-discover problems
    /// later. All three [`EventTiming`] variants (`When`, `At`, `After`)
    /// are exercised independently since unit-variant × serde can fail on
    /// any alone (very rare, but the test rationale explicitly covers this
    /// surface).
    #[test]
    fn on_event_ability_round_trips_through_serde_json() {
        for timing in [EventTiming::When, EventTiming::At, EventTiming::After] {
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

    #[test]
    fn omitting_any_required_ability_field_is_rejected() {
        // `costs` is required on the wire (#453): a payload missing it fails
        // loudly rather than silently defaulting. `usage_limit` is an `Option`
        // and stays implicitly optional (handled separately below).
        let ability = on_event(
            EventPattern::EnemyDefeated {
                by_controller: true,
                code: None,
            },
            EventTiming::When,
            TriggerKind::Reaction,
            discover_clue(LocationTarget::YourLocation, 1),
        );
        let full = serde_json::to_value(&ability).expect("serialize");
        serde_json::from_value::<Ability>(full.clone()).expect("full object deserializes");
        // `costs` is required; omitting it is rejected, not defaulted.
        let mut v = full.clone();
        v.as_object_mut()
            .expect("ability serializes to a JSON object")
            .remove("costs")
            .expect("`costs` present in serialized form");
        assert!(
            serde_json::from_value::<Ability>(v).is_err(),
            "omitting `costs` must be rejected, not defaulted"
        );
        // `usage_limit` stays implicitly optional (it is an `Option`; serde
        // defaults a missing one to `None`) — by design, see its doc.
        let mut v = full;
        v.as_object_mut()
            .unwrap()
            .remove("usage_limit")
            .expect("present in serialized form");
        let back =
            serde_json::from_value::<Ability>(v).expect("absent Option deserializes to None");
        assert!(back.usage_limit.is_none());
    }

    /// `Trigger::ElderSign` is a config-on-trigger variant (like
    /// `Activated { action_cost }`): it carries the elder-sign's printed
    /// modifier as an `IntExpr` and round-trips through serde. Roland's
    /// "+1 for each clue on your location" is `Count(CluesAtControllerLocation)`.
    #[test]
    fn elder_sign_trigger_carries_int_expr_and_round_trips() {
        let t = Trigger::ElderSign {
            modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
        };
        let json = serde_json::to_string(&t).expect("serialize");
        let back: Trigger = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(t, back);
        // Distinct from a literal-modifier elder-sign and from other triggers.
        assert_ne!(
            t,
            Trigger::ElderSign {
                modifier: IntExpr::Lit(1),
            },
        );
        assert_ne!(t, Trigger::Constant);
    }

    /// The `elder_sign` builder produces an [`Ability`] with the correct
    /// trigger, empty costs, empty effect `Seq`, and no usage limit.
    /// Distinct from the `elder_sign_trigger_carries_int_expr_and_round_trips`
    /// test which only exercises the `Trigger` variant itself.
    #[test]
    fn elder_sign_builder_constructs_the_trigger() {
        let a = elder_sign(IntExpr::Count(Quantity::CluesAtControllerLocation));
        assert_eq!(
            a.trigger,
            Trigger::ElderSign {
                modifier: IntExpr::Count(Quantity::CluesAtControllerLocation),
            },
        );
        assert!(a.costs.is_empty());
        assert!(a.usage_limit.is_none());
        assert!(matches!(a.effect, Effect::Seq(ref v) if v.is_empty()));
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
    fn discover_clues_and_game_end_round_trip() {
        for p in [EventPattern::DiscoverClues, EventPattern::GameEnd] {
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
    fn cancel_effect_and_enemy_attacks_pattern_round_trip() {
        let e = Effect::Cancel;
        let json = serde_json::to_string(&e).expect("serialize");
        assert_eq!(
            Effect::Cancel,
            serde_json::from_str(&json).expect("deserialize")
        );

        let p = EventPattern::EnemyAttacks;
        let json = serde_json::to_string(&p).expect("serialize");
        assert_eq!(
            EventPattern::EnemyAttacks,
            serde_json::from_str(&json).expect("deserialize")
        );
    }

    #[test]
    fn skill_test_resolved_round_trips() {
        let p = EventPattern::SkillTestResolved {
            outcome: TestOutcome::Success,
            kind: Some(SkillTestKind::Investigate),
        };
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
