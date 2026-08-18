//! The modified value of a quantity, recalculated from the board at
//! every read.
//!
//! `data/rules-reference/rules/glossary/Modifiers.md`:
//!
//! > The game state constantly checks and (if necessary) updates the
//! > count of any variable value or quantity that is being modified.
//! >
//! > Any time a new modifier is applied (or removed), the entire
//! > quantity is recalculated from the start, considering the unmodified
//! > base value and all active modifiers.
//!
//! [`modified_value`] is that recalculation, and it is the engine's only
//! answer to "what is this quantity right now". It takes a
//! [`ModifierTarget`] and a [`ModifiedQuantity`], sweeps every place a modifying
//! card can sit, and returns a [`ModifierBreakdown`] — the base value
//! plus each contribution attributed to its source — so a total can
//! explain itself rather than merely being an integer.
//!
//! # Population
//!
//! Most modifiers are true exactly while a card sits somewhere, so the
//! sweep over those places *is* the population and there is nothing to
//! keep in sync. Six collections carry them:
//!
//! 1. every investigator's controlled instances (investigator card,
//!    cards in play, threat area),
//! 2. every location's own card,
//! 3. every location's attachments,
//! 4. every enemy's own card,
//! 5. every enemy's attachments,
//! 6. the current act and the current agenda.
//!
//! Narrower is wrong rather than merely incomplete: Whippoorwill 02090
//! is an *enemy* modifying investigators, Whateley Ruins 02250 a
//! *location* doing the same, and The Ritual Begins 01144 an *agenda*
//! modifying every enemy — none of them reachable from a scan keyed on
//! the acting investigator's own cards. Which entity a swept modifier
//! reaches is decided by its [`ModifierAudience`], not by where its
//! source happens to sit.
//!
//! Modifiers whose lifetime is decoupled from any card's zone are
//! *recorded* instead of derived — [`GameState::recorded_modifiers`].
//! Today those are the [`ModifierScope::ThisSkillTest`] row an activated
//! ability pushes and the one-shot modifier an initiating effect grants
//! the test it starts (a weapon's *"+N \[combat\] for this attack"*,
//! Flashlight 01087's *"-2 shroud for this investigation"*). Each carries
//! the [`SkillTestId`](crate::state::SkillTestId) of the test it was bought
//! for and is inert under any other, and each names its own target — so a
//! row can modify a location as readily as an investigator. A recorded row
//! stores its delta as an **expression**, evaluated at read time exactly as
//! a swept modifier's condition is. A row can carry the test's
//! [determination](crate::dsl::Determination) in place of a delta — the
//! `[auto_fail]` chaos token writes one — which is read through
//! [`test_determination`] rather than folded additively.
//!
//! # Difficulty
//!
//! A skill test's difficulty is not a quantity of its own: it *is* the
//! modified shroud of the location being investigated, or the modified
//! fight or evade value of the enemy being attacked or evaded. Reading
//! [`ModifierTarget::Test`] resolves the in-flight test's
//! [`DifficultyBasis`] and asks that target, so every card modifying the
//! enemy or the location modifies the test (#677).
//!
//! # The fold
//!
//! ADR 0005 folds a modified quantity in the Rules Reference's own order
//! — base value, a transform over rows, addition and subtraction,
//! doubling and halving with rounding, then the clamp and whole-quantity
//! substitution. This module implements the base, the additive pass, the
//! clamp and the substitution; the row transform and the multiplicative
//! pass arrive with their first corpus consumers (Jim Culver 02004,
//! Hunting Nightgaunt 01172, Double or Nothing 02026).
//!
//! Substitution — the [determination](test_determination) of automatic
//! failure or automatic success — is the last stage, and the one stage
//! that is not decided inside the fold. The two determinations substitute
//! *different* quantities (the tester's total skill value, the test's
//! total difficulty) and automatic failure takes precedence over
//! automatic success, so a fold evaluating either quantity alone cannot
//! resolve the rule: two independent substitutions would both yield 0 and
//! compare as a success. A test-level query resolves it once and both
//! quantity reads consult it (ADR 0007).
//!
//! The clamp is genuinely last among the additive stages: every
//! contribution to a skill test's ST.5 total is a row this query folds,
//! including the revealed chaos token's ±N and the elder sign's bonus
//! (#684), so nothing is added after [`ModifierBreakdown::total`]
//! returns. That is what `Modifiers.md`'s own worked example demands —
//! base 4, a −8 token and a +2 is −2 → 0, **not** 0 + 2 → 2.

use crate::card_data::SkillKind;
use crate::card_registry::CardRegistry;
use crate::dsl::{
    Determination, Effect, ModifierAudience, ModifierScope, SkillTestKind, Stat, Trigger,
};
use crate::state::{
    CardCode, CardInPlay, DifficultyBasis, EnemyId, GameState, InvestigatorId, LocationId,
    RecordedModifierKind,
};

/// Which entity's quantity is being asked about.
///
/// Defined in [`crate::state`] — a [`RecordedModifier`](crate::state::RecordedModifier)
/// stores one, so it is state rather than query vocabulary — and re-exported
/// here, where it reads as the query's first argument.
pub use crate::state::ModifierTarget;

/// Which quantity of the target is being asked about.
///
/// Mirrors [`Stat`] for the quantities a card can modify, plus
/// [`Difficulty`](Self::Difficulty), which no card names directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifiedQuantity {
    /// One of an investigator's four skills.
    Skill(SkillKind),
    /// An investigator's maximum health, or an enemy's printed health.
    MaxHealth,
    /// An investigator's maximum sanity.
    MaxSanity,
    /// A location's shroud.
    Shroud,
    /// An enemy's fight value.
    Fight,
    /// An enemy's evade value.
    Evade,
    /// The in-flight skill test's difficulty — read against
    /// [`ModifierTarget::Test`], which resolves the test's
    /// [`DifficultyBasis`] and answers from the location or enemy the test
    /// is against.
    Difficulty,
}

/// The evaluation context a read needs: whether it happens inside a
/// skill test, and of what kind.
///
/// This is the whole of the context [`ModifierScope`] can ask about, and
/// it is derivable from the state — see [`ReadContext::from_state`]. A
/// caller passes [`OutsideTest`](Self::OutsideTest) explicitly when it
/// wants only always-on modifiers regardless of what is in flight (prey
/// ranking, which resolves outside any test of its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReadContext {
    /// No skill test bears on the read: only
    /// [`ModifierScope::WhileInPlay`] modifiers apply.
    OutsideTest,
    /// The read happens during a skill test of this kind, so
    /// [`ModifierScope::WhileInPlayDuring`] modifiers matching it apply
    /// too.
    DuringTest(SkillTestKind),
}

impl ReadContext {
    /// The context implied by the state: the in-flight test's kind if
    /// one is in flight, [`OutsideTest`](Self::OutsideTest) otherwise.
    #[must_use]
    pub fn from_state(state: &GameState) -> Self {
        state
            .current_skill_test()
            .map_or(Self::OutsideTest, |t| Self::DuringTest(t.kind))
    }
}

/// What produced one contribution to a modified value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContributionSource {
    /// A card the sweep found on the board. `instance` is `None` for
    /// cards that have no in-play instance of their own — a location, an
    /// enemy, the act, the agenda.
    Card {
        /// The source card's printed code.
        code: CardCode,
        /// The source card's in-play instance, where it has one.
        instance: Option<crate::state::CardInstanceId>,
    },
    /// A recorded row queued by an earlier effect resolution, attributed
    /// to the in-play instance that pushed it where one is known.
    Recorded {
        /// The instance whose ability pushed the row.
        instance: Option<crate::state::CardInstanceId>,
    },
}

/// One modifier's contribution to a modified value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    /// What produced it.
    pub source: ContributionSource,
    /// Its signed magnitude.
    pub delta: i8,
}

/// A modified value and its composition: the base value plus each
/// active modifier attributed to its source.
///
/// The breakdown is a product feature, not a debugging aid — it is what
/// lets a client show why a combat value is 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifierBreakdown {
    /// The base value — the printed number, until a card replaces it
    /// (Duke 02014's *"You attack with a base \[combat\] skill of 4."*,
    /// the fold's stage 1, which no card declares yet).
    pub base: i32,
    /// Every active modifier, in sweep order.
    pub contributions: Vec<Contribution>,
    /// The fold's **stage 5**: a value substituted for the whole quantity,
    /// overriding base, contributions and clamp alike.
    ///
    /// `Some(0)` when the in-flight test's
    /// [determination](test_determination) substitutes *this* quantity —
    /// the tester's total skill value on an automatic failure, the test's
    /// total difficulty on an automatic success
    /// (`glossary/Automatic_Failure_Success.md`). `None` otherwise, which
    /// is every read outside a determined test.
    ///
    /// The contributions are **kept**, not discarded: the rules require the
    /// modified skill value to be determined even on an automatically
    /// failed test (*"the investigator's total modified skill value is
    /// still determined, as it may have some bearing on other card
    /// abilities"*), and the breakdown is where that lives.
    pub substitution: Option<i32>,
}

impl ModifierBreakdown {
    /// Add one more contribution to the fold, attributed to its source.
    ///
    /// For the contributions that are a property of the read rather than of
    /// the board, and so cannot be swept or recorded: the committed cards'
    /// matching and wild icons at RR ST.5. They go *into* the fold rather
    /// than onto [`total`](Self::total)'s result, because the clamp is last
    /// — the whole point of there being no unclamped accessor.
    pub fn push(&mut self, source: ContributionSource, delta: i8) {
        self.contributions.push(Contribution { source, delta });
    }

    /// The modified value: the base plus every contribution, with the
    /// clamp applied last. `Modifiers.md`: *"after all active modifiers
    /// have been applied, any resultant value below zero is treated as
    /// zero"*.
    ///
    /// A [`substitution`](Self::substitution) wins outright — it is the
    /// fold's stage 5, after the clamp, and it replaces the quantity
    /// rather than contributing to it.
    ///
    /// There is no unclamped counterpart, because there is no caller
    /// with modifiers still to add: a contribution that is not on the
    /// board is a [`RecordedModifier`](crate::state::RecordedModifier)
    /// this query already folds.
    #[must_use]
    pub fn total(&self) -> i32 {
        if let Some(substituted) = self.substitution {
            return substituted;
        }
        self.contributions
            .iter()
            .fold(self.base, |acc, c| acc.saturating_add(i32::from(c.delta)))
            .max(0)
    }
}

/// The modified value of `quantity` for `target`, recalculated from the
/// board as it stands.
///
/// `registry` is `None` in engine-only tests with no card data
/// installed; the base value still answers and no modifier contributes.
/// A card in play whose code the registry cannot resolve is skipped
/// silently — the deck-import gate keeps unimplemented codes out of
/// play, so a missing entry means engine-only test data rather than a
/// live card losing its ability.
///
/// A target/quantity pair that does not go together (an investigator's
/// shroud) has base 0 and no contributions: no [`Stat`] maps to it for
/// that target, so nothing can reach it.
#[must_use]
pub fn modified_value(
    state: &GameState,
    registry: Option<&CardRegistry>,
    target: ModifierTarget,
    quantity: ModifiedQuantity,
    context: ReadContext,
) -> ModifierBreakdown {
    let mut breakdown =
        if let (ModifierTarget::Test, ModifiedQuantity::Difficulty) = (target, quantity) {
            difficulty(state, registry, context)
        } else {
            let mut breakdown = ModifierBreakdown {
                base: base_value(state, target, quantity),
                contributions: Vec::new(),
                substitution: None,
            };
            if let Some(registry) = registry {
                sweep(
                    state,
                    registry,
                    target,
                    quantity,
                    context,
                    &mut breakdown.contributions,
                );
            }
            collect_recorded(
                state,
                target,
                quantity,
                context,
                &mut breakdown.contributions,
            );
            breakdown
        };
    // Stage 5, last: the in-flight test's determination substitutes the
    // whole quantity. Applied here rather than inside either branch so the
    // difficulty read gets it too — and after the delegation, so the
    // substitution lands on the *test's* difficulty rather than on the
    // location's shroud or the enemy's fight value it is read from.
    breakdown.substitution = substitution(state, target, quantity, context);
    breakdown
}

/// The in-flight skill test's **determination**, resolved across every
/// recorded row scoped to it: automatically failed, automatically
/// succeeded, or neither.
///
/// This is the query ADR 0007 puts *above* the fold. The precedence —
/// automatic failure beats automatic success — is a rule spanning two
/// quantities, and a fold evaluating either one cannot see the other's
/// rows; two independent stage-5 substitutions would both yield 0 and
/// compare as a success, which is the wrong answer.
///
/// Resolved at **read** time, so both rows may coexist: neither suppresses
/// nor overwrites the other, and the answer does not depend on which was
/// latched first.
///
/// `None` when the read declares itself outside a test
/// ([`ReadContext::OutsideTest`], as prey ranking does) or when no test is
/// in flight — the same gate the recorded-row collector applies, since a
/// determination *is* one of those rows.
#[must_use]
pub fn test_determination(state: &GameState, context: ReadContext) -> Option<Determination> {
    let ReadContext::DuringTest(_) = context else {
        return None;
    };
    let in_flight = state.current_skill_test().map(|t| t.id)?;
    let mut found = None;
    for row in &state.recorded_modifiers {
        let RecordedModifierKind::Determination(d) = row.kind else {
            continue;
        };
        if !row.lifetime.applies_during_test(in_flight) {
            continue;
        }
        match d {
            Determination::AutomaticFailure => return Some(Determination::AutomaticFailure),
            Determination::AutomaticSuccess => found = Some(Determination::AutomaticSuccess),
        }
    }
    found
}

/// The fold's stage-5 substitution for one `target`/`quantity` pair:
/// `Some(0)` when the test's determination replaces this quantity
/// wholesale, `None` otherwise.
///
/// `glossary/Automatic_Failure_Success.md`:
///
/// > - If a skill test automatically fails, the investigator's total skill
/// >   value for that test is considered 0.
/// > - If a skill test automatically succeeds, the total difficulty of
/// >   that test is considered 0.
///
/// The failure clause is narrowed to the tester's *tested* skill, which is
/// the only quantity the rule names ("the investigator's total skill value
/// **for that test**"): a determined test does not zero a bystander's
/// willpower, nor the tester's other three skills.
fn substitution(
    state: &GameState,
    target: ModifierTarget,
    quantity: ModifiedQuantity,
    context: ReadContext,
) -> Option<i32> {
    let determination = test_determination(state, context)?;
    let test = state.current_skill_test()?;
    match determination {
        Determination::AutomaticFailure
            if target == ModifierTarget::Investigator(test.investigator)
                && quantity == ModifiedQuantity::Skill(test.skill) =>
        {
            Some(0)
        }
        Determination::AutomaticSuccess
            if target == ModifierTarget::Test && quantity == ModifiedQuantity::Difficulty =>
        {
            Some(0)
        }
        _ => None,
    }
}

/// The in-flight test's difficulty: whatever its
/// [`DifficultyBasis`] names, read as the board stands **now**.
///
/// An enemy's modified fight value *is* the difficulty of a Fight
/// action — [`DifficultyBasis`] carries the rule and its citation — so this
/// delegates rather than folding a difficulty of its own:
/// every card modifying that enemy's fight modifies the test, and the clamp
/// happens once, in the delegate's [`ModifierBreakdown::total`].
///
/// [`Fixed`](DifficultyBasis::Fixed) — a treachery's printed difficulty —
/// is a base value with no contributions; a card that modifies a card-test's
/// difficulty (Double or Nothing 02026 doubles it) wants the fold's
/// multiplicative stage, which is not built.
///
/// With no test in flight there is no difficulty to read: base 0, no
/// contributions.
///
/// **TODO(#682):** the same is true if the basis names an entity that has
/// left play — an enemy defeated between ST.1 and ST.6 — and there the
/// answer is wrong rather than merely absent: difficulty 0 makes the attack
/// automatically succeed. The vendored rules do not say what becomes of a
/// test whose target leaves play, so #677 declined to invent an answer;
/// #682 owns the ruling.
fn difficulty(
    state: &GameState,
    registry: Option<&CardRegistry>,
    context: ReadContext,
) -> ModifierBreakdown {
    let Some(basis) = state.current_skill_test().map(|t| t.difficulty_basis) else {
        return ModifierBreakdown {
            base: 0,
            contributions: Vec::new(),
            substitution: None,
        };
    };
    let (target, quantity) = match basis {
        DifficultyBasis::Fixed(n) => {
            return ModifierBreakdown {
                base: i32::from(n),
                contributions: Vec::new(),
                substitution: None,
            }
        }
        DifficultyBasis::Shroud(id) => (ModifierTarget::Location(id), ModifiedQuantity::Shroud),
        DifficultyBasis::Fight(id) => (ModifierTarget::Enemy(id), ModifiedQuantity::Fight),
        DifficultyBasis::Evade(id) => (ModifierTarget::Enemy(id), ModifiedQuantity::Evade),
    };
    modified_value(state, registry, target, quantity, context)
}

/// Stage 1: the printed value. Base replacement (Duke 02014) lands with
/// that card; until then the base is always what the card prints.
fn base_value(state: &GameState, target: ModifierTarget, quantity: ModifiedQuantity) -> i32 {
    match (target, quantity) {
        (ModifierTarget::Investigator(id), ModifiedQuantity::Skill(skill)) => state
            .investigators
            .get(&id)
            .map_or(0, |inv| i32::from(inv.skills.value(skill))),
        (ModifierTarget::Investigator(id), ModifiedQuantity::MaxHealth) => state
            .investigators
            .get(&id)
            .map_or(0, |inv| i32::from(inv.max_health())),
        (ModifierTarget::Investigator(id), ModifiedQuantity::MaxSanity) => state
            .investigators
            .get(&id)
            .map_or(0, |inv| i32::from(inv.max_sanity())),
        (ModifierTarget::Location(id), ModifiedQuantity::Shroud) => state
            .locations
            .get(&id)
            .map_or(0, |loc| i32::from(loc.shroud)),
        (ModifierTarget::Enemy(id), ModifiedQuantity::Fight) => {
            state.enemies.get(&id).map_or(0, |e| i32::from(e.fight))
        }
        (ModifierTarget::Enemy(id), ModifiedQuantity::Evade) => {
            state.enemies.get(&id).map_or(0, |e| i32::from(e.evade))
        }
        (ModifierTarget::Enemy(id), ModifiedQuantity::MaxHealth) => state
            .enemies
            .get(&id)
            .map_or(0, |e| i32::from(e.max_health)),
        (ModifierTarget::Test, ModifiedQuantity::Difficulty) => unreachable!(
            "modified_value resolves the difficulty basis and asks the underlying \
             target; base_value never sees the pair"
        ),
        _ => 0,
    }
}

/// Where a swept source card sits. Decides both which entity an
/// audience resolves against and which location counts as "the source's
/// location".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// An instance an investigator controls: their investigator card, a
    /// card in play, or a card in their threat area.
    Controlled(InvestigatorId),
    /// A location's own card.
    Location(LocationId),
    /// A card attached to a location.
    LocationAttachment(LocationId),
    /// An enemy's own card.
    Enemy(EnemyId),
    /// A card attached to an enemy.
    EnemyAttachment(EnemyId),
    /// The current act or agenda. Has no location of its own, so
    /// location-scoped audiences never resolve from one.
    ActAgenda,
}

/// Sweep the six collections, pushing every active modifier that
/// reaches `target`.
///
/// Matches a **bare** `Effect::Modify` under `Trigger::Constant`. A
/// modifier gated on a predicate — `Effect::If { condition, then:
/// Modify }` — is skipped, carrying forward the gap the superseded
/// `constant_skill_modifier` had. That is a card-expressiveness gap
/// rather than a read-time one: everything matched here is re-derived
/// at every read. Wiring it needs `Condition` variants no card can
/// express yet *and* a decision about who "you" is for a source with no
/// controller, so it is **#679**.
fn sweep(
    state: &GameState,
    registry: &CardRegistry,
    target: ModifierTarget,
    quantity: ModifiedQuantity,
    context: ReadContext,
    out: &mut Vec<Contribution>,
) {
    let mut visit = |code: &CardCode, instance: Option<&CardInPlay>, placement: Placement| {
        let Some(abilities) = (registry.abilities_for)(code) else {
            return;
        };
        for ability in &abilities {
            if ability.trigger != Trigger::Constant {
                continue;
            }
            let Effect::Modify {
                stat,
                delta,
                scope,
                audience,
            } = &ability.effect
            else {
                continue;
            };
            if !scope_applies(*scope, context)
                || !stat_matches(*stat, quantity)
                || !audience_reaches(state, placement, *audience, target)
            {
                continue;
            }
            out.push(Contribution {
                source: ContributionSource::Card {
                    code: code.clone(),
                    instance: instance.map(|c| c.instance_id),
                },
                delta: *delta,
            });
        }
    };

    // 1. Every investigator's controlled instances — not just the
    //    target's, so Lita Chantler 01117 can reach a teammate.
    for inv in state.investigators.values() {
        for card in inv.controlled_card_instances() {
            visit(&card.code, Some(card), Placement::Controlled(inv.id));
        }
    }
    // 2 and 3. Every location and its attachments.
    for (id, loc) in &state.locations {
        visit(&loc.code, None, Placement::Location(*id));
        for att in &loc.attachments {
            visit(&att.code, Some(att), Placement::LocationAttachment(*id));
        }
    }
    // 4 and 5. Every enemy and its attachments.
    for (id, enemy) in &state.enemies {
        visit(&enemy.code, None, Placement::Enemy(*id));
        for att in &enemy.attachments {
            visit(&att.code, Some(att), Placement::EnemyAttachment(*id));
        }
    }
    // 6. The current act and agenda.
    if let Some(act) = state.act_deck.get(state.act_index) {
        visit(&act.code, None, Placement::ActAgenda);
    }
    if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
        visit(&agenda.code, None, Placement::ActAgenda);
    }
}

/// The recorded half of the population: rows whose lifetime is
/// decoupled from any card's zone.
///
/// Every row today carries [`Lifetime::SkillTest`](crate::state::Lifetime::SkillTest), so it contributes
/// only while the test it names is the test in flight — a row bought for
/// an earlier test is **inert**, not merely drained, and a read that
/// declares itself outside a test (prey ranking, which passes
/// [`ReadContext::OutsideTest`] explicitly) sees none of them.
///
/// A row names its own [`target`](crate::state::RecordedModifier::target),
/// so it is not restricted to the investigator who bought it: the shroud
/// reduction Flashlight 01087 grants the investigation it starts (*"Your
/// location gets -2 shroud for this investigation."*) is a row over that
/// **location**, folded into the same query the location's attachments feed
/// (Obscuring Fog 01168's *"Attached location gets +2 shroud."*) — one
/// composed shroud, clamped once.
///
/// The delta is an expression evaluated **here**, at read time, against
/// the row's investigator as "you". A `Modify` writes an
/// [`IntExpr::Lit`](crate::dsl::IntExpr::Lit), as does the revealed chaos
/// token's ±N; the elder-sign row carries the investigator card's own
/// expression, so Roland Banks 01001's *"+1 for each clue on your
/// location"* counts the clues that are there at ST.5 rather than the ones
/// that were there at the reveal (#684).
///
/// An expression that cannot be resolved is **skipped**, not counted as
/// zero and not asserted on: contributing a silently-wrong number is worse
/// than contributing none, and a malformed elder sign (an `IntExpr` over a
/// `Condition` the evaluator cannot express) is card data rather than an
/// engine invariant, so it must not panic mid-test. That is the guard the
/// superseded `elder_sign_modifier` carried in its own `unwrap_or(0)`.
fn collect_recorded(
    state: &GameState,
    target: ModifierTarget,
    quantity: ModifiedQuantity,
    context: ReadContext,
    out: &mut Vec<Contribution>,
) {
    let ReadContext::DuringTest(_) = context else {
        return;
    };
    let Some(in_flight) = state.current_skill_test().map(|t| t.id) else {
        return;
    };
    for row in &state.recorded_modifiers {
        // A determination row is not an additive contribution: it is the
        // fold's stage-5 substitution, read through `test_determination`
        // once for the whole test rather than folded per quantity.
        let RecordedModifierKind::Delta { stat, ref delta } = row.kind else {
            continue;
        };
        if row.target != target
            || !stat_matches(stat, quantity)
            || !row.lifetime.applies_during_test(in_flight)
        {
            continue;
        }
        let eval_ctx = crate::engine::evaluator::EvalContext::for_controller_with_optional_source(
            row.investigator,
            row.source,
        );
        let Ok(delta) = crate::engine::evaluator::eval_int_expr(state, &eval_ctx, delta) else {
            continue;
        };
        out.push(Contribution {
            source: ContributionSource::Recorded {
                instance: row.source,
            },
            delta,
        });
    }
}

/// Whether a constant-trigger [`ModifierScope`] contributes under
/// `context`.
///
/// [`WhileInPlay`](ModifierScope::WhileInPlay) is unqualified — it
/// applies to every read. [`WhileInPlayDuring`](ModifierScope::WhileInPlayDuring)
/// needs a test of the matching kind (Magnifying Glass 01030's *"+1
/// \[intellect\] while investigating"*). The non-constant scopes are
/// recorded rows, never swept.
fn scope_applies(scope: ModifierScope, context: ReadContext) -> bool {
    match scope {
        ModifierScope::WhileInPlay => true,
        ModifierScope::WhileInPlayDuring(kind) => context == ReadContext::DuringTest(kind),
        ModifierScope::ThisSkillTest | ModifierScope::ThisTurn => false,
    }
}

/// Whether a DSL [`Stat`] names the quantity being asked about.
fn stat_matches(stat: Stat, quantity: ModifiedQuantity) -> bool {
    match quantity {
        ModifiedQuantity::Skill(skill) => stat == stat_for_skill(skill),
        ModifiedQuantity::MaxHealth => stat == Stat::MaxHealth,
        ModifiedQuantity::MaxSanity => stat == Stat::MaxSanity,
        ModifiedQuantity::Shroud => stat == Stat::Shroud,
        ModifiedQuantity::Fight => stat == Stat::Fight,
        ModifiedQuantity::Evade => stat == Stat::Evade,
        // No card names a test's difficulty; it is modified by
        // modifying the location or enemy the test is against.
        ModifiedQuantity::Difficulty => false,
    }
}

/// Whether a modifier declared with `audience`, on a source sitting at
/// `placement`, reaches `target`.
fn audience_reaches(
    state: &GameState,
    placement: Placement,
    audience: ModifierAudience,
    target: ModifierTarget,
) -> bool {
    match (audience, target) {
        (ModifierAudience::Controller, ModifierTarget::Investigator(id)) => {
            placement == Placement::Controlled(id)
        }
        (ModifierAudience::EachInvestigatorAtSourceLocation, ModifierTarget::Investigator(id)) => {
            let Some(here) = source_location(state, placement) else {
                return false;
            };
            state
                .investigators
                .get(&id)
                .is_some_and(|inv| inv.current_location == Some(here))
        }
        (ModifierAudience::EachEnemyAtSourceLocation, ModifierTarget::Enemy(id)) => {
            let Some(here) = source_location(state, placement) else {
                return false;
            };
            state
                .enemies
                .get(&id)
                .is_some_and(|enemy| enemy.current_location == Some(here))
        }
        (ModifierAudience::EachEnemy, ModifierTarget::Enemy(_)) => true,
        (ModifierAudience::AttachedCard, ModifierTarget::Location(id)) => {
            placement == Placement::LocationAttachment(id)
        }
        (ModifierAudience::AttachedCard, ModifierTarget::Enemy(id)) => {
            placement == Placement::EnemyAttachment(id)
        }
        _ => false,
    }
}

/// The location a source card counts as being at: its controller's for a
/// controlled card, the location itself for a location or its
/// attachments, the enemy's for an enemy or its attachments. The act and
/// agenda are nowhere, and so reach no location-scoped audience.
fn source_location(state: &GameState, placement: Placement) -> Option<LocationId> {
    match placement {
        Placement::Controlled(id) => state
            .investigators
            .get(&id)
            .and_then(|inv| inv.current_location),
        Placement::Location(id) | Placement::LocationAttachment(id) => Some(id),
        Placement::Enemy(id) | Placement::EnemyAttachment(id) => state
            .enemies
            .get(&id)
            .and_then(|enemy| enemy.current_location),
        Placement::ActAgenda => None,
    }
}

/// The controller's **elder-sign** skill-test modifier as an
/// [`IntExpr`](crate::dsl::IntExpr): the expression on their investigator
/// card's [`Trigger::ElderSign`] ability, **copied unevaluated**.
///
/// Lives here rather than in the evaluator because it answers the same
/// question as [`modified_value`] — what is contributing to this
/// investigator's total right now — for the one contributor the sweep
/// cannot see: the revealed chaos token. When an `[elder_sign]` is
/// revealed at ST.3 the skill-test driver records this expression as a
/// [`RecordedModifier`](crate::state::RecordedModifier) scoped to that
/// test, and [`collect_recorded`] evaluates it at ST.5 like every other
/// row (#684).
///
/// The expression must **not** be resolved to a literal at reveal time.
/// Roland Banks 01001 (*"`[elder_sign]` effect: +1 for each clue on your
/// location."*) is the corpus consumer, and freezing his clue count at the
/// reveal would pin it across the ST.4 window — the staleness ADR 0005
/// exists to kill. A sweep of the snapshot found twelve investigators with
/// a state-contingent elder-sign modifier, so it is not a one-card concern
/// (ADR 0007).
///
/// `None` when the controller is not found, the card isn't in the
/// registry, or it carries no elder-sign ability — so every investigator
/// without an elder-sign contributes no row at all rather than a zero one.
///
/// **Scope (#118), sunset by #448:** handles only pure-modifier
/// elder-signs. Signs that also run an effect (Daisy / Agnes) are
/// deferred — see [`Trigger::ElderSign`].
#[must_use]
pub(crate) fn elder_sign_expr(
    state: &GameState,
    registry: &CardRegistry,
    controller: InvestigatorId,
) -> Option<crate::dsl::IntExpr> {
    let inv = state.investigators.get(&controller)?;
    let abilities = (registry.abilities_for)(&inv.investigator_card.code)?;
    abilities.iter().find_map(|ability| match &ability.trigger {
        Trigger::ElderSign { modifier } => Some(modifier.clone()),
        _ => None,
    })
}

/// The [`Stat`] a tested [`SkillKind`] names.
///
/// The one place the two vocabularies are mapped: [`stat_matches`] reads it
/// to *filter* rows by the quantity being asked about, and the skill-test
/// driver reads it to *write* a row against the skill a test is being taken
/// with (the revealed token's ±N and the elder sign's bonus, #684).
#[must_use]
pub(crate) fn stat_for_skill(skill: SkillKind) -> Stat {
    match skill {
        SkillKind::Willpower => Stat::Willpower,
        SkillKind::Intellect => Stat::Intellect,
        SkillKind::Combat => Stat::Combat,
        SkillKind::Agility => Stat::Agility,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::{constant, elder_sign, modify, modify_for, on_play, Ability, IntExpr};
    use crate::state::{CardInstanceId, LocationId};
    use crate::test_support::{
        test_enemy, test_investigator, test_location, test_skill_test, GameStateBuilder,
    };

    /// Mock registry over a small hardcoded set of codes. Keeps these
    /// tests isolated from the global `OnceLock` and from the cards
    /// crate — a query takes its registry by argument, so nothing is
    /// installed. Named to match `tests/modified_value.rs`'s mocks,
    /// which cover the same sweep from the integration side.
    fn mock_metadata_for(_: &CardCode) -> Option<&'static crate::card_data::CardMetadata> {
        None
    }

    fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
        match code.as_str() {
            "willpower-plus-1" => Some(vec![constant(modify(
                Stat::Willpower,
                1,
                ModifierScope::WhileInPlay,
            ))]),
            "willpower-minus-1" => Some(vec![constant(modify(
                Stat::Willpower,
                -1,
                ModifierScope::WhileInPlay,
            ))]),
            "intellect-plus-2" => Some(vec![constant(modify(
                Stat::Intellect,
                2,
                ModifierScope::WhileInPlay,
            ))]),
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
            // Obscuring Fog 01168's shape: "Attached location gets +2
            // shroud."
            "shroud-plus-2" => Some(vec![constant(modify_for(
                ModifierAudience::AttachedCard,
                Stat::Shroud,
                2,
                ModifierScope::WhileInPlay,
            ))]),
            "elder-sign-clues-here" => Some(vec![elder_sign(IntExpr::Count(
                crate::dsl::Quantity::CluesAtControllerLocation,
            ))]),
            _ => None,
        }
    }

    fn mock_registry() -> CardRegistry {
        CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: |_| None,
            native_eligibility_for: |_| None,
            native_condition_for: |_| None,
        }
    }

    fn state_with_cards_in_play(codes: &[&str]) -> (GameState, InvestigatorId) {
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

    /// `modified_value` for an investigator's skill, in a plain test.
    fn skill(state: &GameState, id: InvestigatorId, skill: SkillKind) -> i32 {
        modified_value(
            state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(id),
            ModifiedQuantity::Skill(skill),
            ReadContext::DuringTest(SkillTestKind::Plain),
        )
        .total()
    }

    // ---- the fold's arithmetic -----------------------------------

    /// `Modifiers.md`'s own worked example: *"Danny's agility would
    /// then be calculated as follows: base skill 4, –8 from chaos token,
    /// +2 from "Lucky!" for a total of –2, which is still treated as
    /// zero."* The clamp is last, so it must not swallow the +2 that
    /// arrives after the −8.
    #[test]
    fn the_clamp_is_applied_once_after_every_modifier() {
        let breakdown = ModifierBreakdown {
            base: 4,
            contributions: vec![
                Contribution {
                    source: ContributionSource::Recorded { instance: None },
                    delta: -8,
                },
                Contribution {
                    source: ContributionSource::Recorded { instance: None },
                    delta: 2,
                },
            ],
            substitution: None,
        };
        assert_eq!(breakdown.total(), 0, "4 - 8 + 2 = -2, treated as 0");
        // The answer a fold that clamped between the −8 and the +2 would
        // give, spelled out so the assertion below still discriminates:
        // (4 − 8 → 0) + 2 = 2.
        let clamping_early = 4_i32.saturating_add(-8).max(0).saturating_add(2);
        assert_eq!(clamping_early, 2, "the wrong fold's arithmetic");
        assert_ne!(
            breakdown.total(),
            clamping_early,
            "clamping before the +2 would give 2 — the answer the rules \
             reference explicitly rules out"
        );
    }

    #[test]
    fn a_breakdown_with_no_contributions_is_its_base() {
        let (state, id) = state_with_cards_in_play(&[]);
        let breakdown = modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(id),
            ModifiedQuantity::Skill(SkillKind::Willpower),
            ReadContext::DuringTest(SkillTestKind::Plain),
        );
        assert_eq!(breakdown.base, 3, "test_investigator's printed willpower");
        assert!(breakdown.contributions.is_empty());
        assert_eq!(breakdown.total(), 3);
    }

    /// The breakdown names each contribution's source, so a client can
    /// show why a value is what it is.
    #[test]
    fn a_breakdown_attributes_each_contribution_to_its_source() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1", "willpower-minus-1"]);
        let breakdown = modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(id),
            ModifiedQuantity::Skill(SkillKind::Willpower),
            ReadContext::DuringTest(SkillTestKind::Plain),
        );
        assert_eq!(breakdown.base, 3);
        assert_eq!(
            breakdown.contributions,
            vec![
                Contribution {
                    source: ContributionSource::Card {
                        code: CardCode::new("willpower-plus-1"),
                        instance: Some(CardInstanceId(0)),
                    },
                    delta: 1,
                },
                Contribution {
                    source: ContributionSource::Card {
                        code: CardCode::new("willpower-minus-1"),
                        instance: Some(CardInstanceId(1)),
                    },
                    delta: -1,
                },
            ],
        );
        assert_eq!(breakdown.total(), 3);
    }

    // ---- what the sweep counts -----------------------------------

    /// The investigator card lives in `investigator_card`, not in
    /// `cards_in_play`; the sweep walks `controlled_card_instances()`,
    /// which yields it first.
    #[test]
    fn a_seated_investigator_cards_modifier_is_counted() {
        let (mut state, id) = state_with_cards_in_play(&[]);
        state
            .investigators
            .get_mut(&id)
            .unwrap()
            .investigator_card
            .code = CardCode::new("inv-willpower-plus-2");
        assert!(
            state.investigators[&id].cards_in_play.is_empty(),
            "the modifier must come from the investigator card, not cards_in_play"
        );
        assert_eq!(skill(&state, id, SkillKind::Willpower), 5);
    }

    #[test]
    fn matching_contributions_are_summed() {
        let (state, id) =
            state_with_cards_in_play(&["willpower-plus-1", "willpower-plus-1", "willpower-plus-1"]);
        assert_eq!(skill(&state, id, SkillKind::Willpower), 6);
    }

    #[test]
    fn a_modifier_to_another_skill_does_not_contribute() {
        let (state, id) = state_with_cards_in_play(&["intellect-plus-2"]);
        assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
        assert_eq!(skill(&state, id, SkillKind::Intellect), 5);
    }

    /// `ThisSkillTest` is a recorded scope: a card declaring it under
    /// `Trigger::Constant` is not swept.
    #[test]
    fn a_non_constant_scope_is_not_swept() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1-this-test-only"]);
        assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
    }

    /// An `OnPlay` Modify resolved once when the card was played; it is
    /// not a standing contribution.
    #[test]
    fn a_non_constant_trigger_is_not_swept() {
        let (state, id) = state_with_cards_in_play(&["non-constant-willpower"]);
        assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
    }

    #[test]
    fn a_capacity_modifier_never_lands_on_a_skill() {
        let (state, id) = state_with_cards_in_play(&["max-health-plus-1"]);
        for kind in [
            SkillKind::Willpower,
            SkillKind::Intellect,
            SkillKind::Combat,
            SkillKind::Agility,
        ] {
            assert_eq!(skill(&state, id, kind), 3, "{kind:?}");
        }
    }

    /// Max health reads the printed capacity off the installed registry
    /// and folds the same sweep over it.
    #[test]
    fn a_capacity_modifier_lands_on_max_health() {
        crate::test_support::install_test_registry();
        let (state, id) = state_with_cards_in_play(&["max-health-plus-1", "willpower-plus-1"]);
        let health = modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(id),
            ModifiedQuantity::MaxHealth,
            ReadContext::OutsideTest,
        );
        assert_eq!(health.base, 8, "TEST_INV's printed health");
        assert_eq!(health.total(), 9, "the willpower buff must not leak in");
    }

    /// A card in play whose code the registry can't resolve is skipped
    /// silently — the deck-import gate keeps unimplemented codes out of
    /// play, so an unresolvable code is engine-only test data.
    #[test]
    fn an_unknown_code_is_skipped() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1", "unknown-card"]);
        assert_eq!(skill(&state, id, SkillKind::Willpower), 4);
    }

    #[test]
    fn an_unknown_investigator_has_no_value_at_all() {
        let state = GameStateBuilder::new().build();
        let breakdown = modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(InvestigatorId(99)),
            ModifiedQuantity::Skill(SkillKind::Willpower),
            ReadContext::DuringTest(SkillTestKind::Plain),
        );
        assert_eq!(breakdown.base, 0);
        assert!(breakdown.contributions.is_empty());
    }

    #[test]
    fn with_no_registry_only_the_base_answers() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
        let breakdown = modified_value(
            &state,
            None,
            ModifierTarget::Investigator(id),
            ModifiedQuantity::Skill(SkillKind::Willpower),
            ReadContext::DuringTest(SkillTestKind::Plain),
        );
        assert_eq!(breakdown.base, 3);
        assert!(breakdown.contributions.is_empty());
    }

    // ---- scope and read context ----------------------------------

    /// A Magnifying-Glass-shaped card: *"+1 \[intellect\] while
    /// investigating."* Contributes during an Investigate test and
    /// nowhere else.
    #[test]
    fn a_during_scope_contributes_only_to_its_own_test_kind() {
        let (state, id) = state_with_cards_in_play(&["intellect-plus-1-while-investigating"]);
        let read = |context| {
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Investigator(id),
                ModifiedQuantity::Skill(SkillKind::Intellect),
                context,
            )
            .total()
        };
        assert_eq!(read(ReadContext::DuringTest(SkillTestKind::Investigate)), 4);
        assert_eq!(read(ReadContext::DuringTest(SkillTestKind::Plain)), 3);
        assert_eq!(read(ReadContext::DuringTest(SkillTestKind::Fight)), 3);
        assert_eq!(read(ReadContext::OutsideTest), 3);
    }

    /// Holy-Rosary-shaped: unqualified `WhileInPlay` contributes to
    /// every read, inside a test or out of one.
    #[test]
    fn an_unqualified_scope_contributes_to_every_read() {
        let (state, id) = state_with_cards_in_play(&["willpower-plus-1"]);
        for context in [
            ReadContext::OutsideTest,
            ReadContext::DuringTest(SkillTestKind::Investigate),
            ReadContext::DuringTest(SkillTestKind::Fight),
            ReadContext::DuringTest(SkillTestKind::Evade),
            ReadContext::DuringTest(SkillTestKind::Plain),
        ] {
            assert_eq!(
                modified_value(
                    &state,
                    Some(&mock_registry()),
                    ModifierTarget::Investigator(id),
                    ModifiedQuantity::Skill(SkillKind::Willpower),
                    context,
                )
                .total(),
                4,
                "WhileInPlay should apply in {context:?}",
            );
        }
    }

    /// Scope and stat are independent filters: the right test kind does
    /// not make a modifier apply to the wrong skill.
    #[test]
    fn a_during_scope_still_respects_the_stat() {
        let (state, id) = state_with_cards_in_play(&["intellect-plus-1-while-investigating"]);
        assert_eq!(
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Investigator(id),
                ModifiedQuantity::Skill(SkillKind::Willpower),
                ReadContext::DuringTest(SkillTestKind::Investigate),
            )
            .total(),
            3,
        );
    }

    // ---- recorded rows -------------------------------------------

    /// The id of the test the recorded-row tests put in flight.
    const IN_FLIGHT: crate::state::SkillTestId = crate::state::SkillTestId(7);

    /// One investigator, `rows` recorded, and the test identified by
    /// [`IN_FLIGHT`] in flight — the shape a `ThisSkillTest` row is read
    /// under.
    fn state_with_recorded(rows: Vec<crate::state::RecordedModifier>) -> GameState {
        state_with_recorded_during(rows, IN_FLIGHT)
    }

    /// As [`state_with_recorded`], with the in-flight test's id chosen by
    /// the caller — so a row can be read against a *different* test.
    fn state_with_recorded_during(
        rows: Vec<crate::state::RecordedModifier>,
        in_flight: crate::state::SkillTestId,
    ) -> GameState {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.recorded_modifiers = rows;
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(test_skill_test(
                in_flight,
                InvestigatorId(1),
                SkillKind::Willpower,
                SkillTestKind::Plain,
                2,
            )));
        state
    }

    /// A row scoped to [`IN_FLIGHT`].
    fn recorded(
        investigator: InvestigatorId,
        stat: Stat,
        delta: i8,
    ) -> crate::state::RecordedModifier {
        recorded_for(investigator, stat, delta, IN_FLIGHT)
    }

    /// A row scoped to the named test.
    fn recorded_for(
        investigator: InvestigatorId,
        stat: Stat,
        delta: i8,
        test: crate::state::SkillTestId,
    ) -> crate::state::RecordedModifier {
        crate::state::RecordedModifier::new(
            investigator,
            stat,
            crate::dsl::IntExpr::Lit(delta),
            crate::state::Lifetime::SkillTest(test),
            None,
        )
    }

    #[test]
    fn recorded_rows_for_the_target_are_summed() {
        let id = InvestigatorId(1);
        let state = state_with_recorded(vec![
            recorded(id, Stat::Intellect, 1),
            recorded(id, Stat::Intellect, 2),
        ]);
        assert_eq!(skill(&state, id, SkillKind::Intellect), 6);
    }

    #[test]
    fn a_recorded_row_for_another_investigator_is_ignored() {
        let state = state_with_recorded(vec![recorded(InvestigatorId(2), Stat::Willpower, 5)]);
        assert_eq!(skill(&state, InvestigatorId(1), SkillKind::Willpower), 3);
    }

    #[test]
    fn a_recorded_row_for_another_stat_is_ignored() {
        let id = InvestigatorId(1);
        let state = state_with_recorded(vec![
            recorded(id, Stat::Intellect, 1),
            recorded(id, Stat::MaxHealth, 1),
        ]);
        assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
    }

    /// The identity check, and the reason it exists: a row bought for one
    /// test contributes nothing to another. Not merely "it was drained in
    /// time" — a row that somehow survived its test is **inert**.
    #[test]
    fn a_recorded_row_from_another_test_contributes_nothing() {
        let id = InvestigatorId(1);
        let state = state_with_recorded_during(
            vec![recorded_for(
                id,
                Stat::Willpower,
                5,
                crate::state::SkillTestId(1),
            )],
            crate::state::SkillTestId(2),
        );
        assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
    }

    /// A `ThisSkillTest` row is scoped to a test, so a read that declares
    /// itself outside one — prey ranking — must not see it, even with
    /// that very test in flight.
    #[test]
    fn a_recorded_row_is_invisible_to_an_outside_test_read() {
        let id = InvestigatorId(1);
        let state = state_with_recorded(vec![recorded(id, Stat::Willpower, 5)]);
        assert_eq!(
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Investigator(id),
                ModifiedQuantity::Skill(SkillKind::Willpower),
                ReadContext::OutsideTest,
            )
            .total(),
            3,
        );
    }

    /// With no test in flight there is no id to match, so a stray row
    /// cannot contribute however the read describes itself.
    #[test]
    fn a_recorded_row_contributes_nothing_with_no_test_in_flight() {
        let id = InvestigatorId(1);
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.recorded_modifiers = vec![recorded(id, Stat::Willpower, 5)];
        assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
    }

    /// A row's delta is an expression resolved at read time, not a number
    /// frozen when the row was pushed: a `Count` row answers from the
    /// board as it stands at the read.
    #[test]
    fn a_recorded_rows_delta_is_evaluated_at_read_time() {
        use crate::dsl::{IntExpr, Quantity};
        let id = InvestigatorId(1);
        let loc = LocationId(3);
        let mut state = state_with_recorded(vec![crate::state::RecordedModifier::new(
            id,
            Stat::Willpower,
            IntExpr::Count(Quantity::CluesAtControllerLocation),
            crate::state::Lifetime::SkillTest(IN_FLIGHT),
            None,
        )]);
        state.locations.insert(loc, test_location(3, "Study"));
        state.investigators.get_mut(&id).unwrap().current_location = Some(loc);
        state.locations.get_mut(&loc).unwrap().clues = 2;
        assert_eq!(skill(&state, id, SkillKind::Willpower), 5);
        // Same row, different board: the answer moves with it, with no
        // invalidation step in between.
        state.locations.get_mut(&loc).unwrap().clues = 4;
        assert_eq!(skill(&state, id, SkillKind::Willpower), 7);
    }

    // ---- non-investigator targets --------------------------------

    #[test]
    fn a_locations_shroud_folds_in_its_attachments() {
        let mut loc = test_location(3, "Study"); // printed shroud 2
        loc.attachments.push(CardInPlay::enter_play(
            CardCode::new("shroud-plus-2"),
            CardInstanceId(0),
        ));
        let state = GameStateBuilder::new().with_location(loc).build();
        let shroud = modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Location(LocationId(3)),
            ModifiedQuantity::Shroud,
            ReadContext::OutsideTest,
        );
        assert_eq!(shroud.base, 2);
        assert_eq!(shroud.total(), 4);
    }

    #[test]
    fn a_locations_shroud_with_no_attachments_is_the_printed_value() {
        let state = GameStateBuilder::new()
            .with_location(test_location(3, "Study"))
            .build();
        assert_eq!(
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Location(LocationId(3)),
                ModifiedQuantity::Shroud,
                ReadContext::OutsideTest,
            )
            .total(),
            2,
        );
    }

    /// An `AttachedCard` modifier reaches only the entity its source is
    /// attached to — a second location in play is unaffected.
    #[test]
    fn an_attached_card_reaches_only_what_it_is_attached_to() {
        let mut fogged = test_location(3, "Study");
        fogged.attachments.push(CardInPlay::enter_play(
            CardCode::new("shroud-plus-2"),
            CardInstanceId(0),
        ));
        let state = GameStateBuilder::new()
            .with_location(fogged)
            .with_location(test_location(4, "Hallway"))
            .build();
        let shroud = |id| {
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Location(id),
                ModifiedQuantity::Shroud,
                ReadContext::OutsideTest,
            )
            .total()
        };
        assert_eq!(shroud(LocationId(3)), 4);
        assert_eq!(shroud(LocationId(4)), 2);
    }

    #[test]
    fn an_enemys_fight_and_evade_answer_from_their_printed_values() {
        let state = GameStateBuilder::new()
            .with_enemy(test_enemy(7, "Ghoul"))
            .build();
        let read = |quantity| {
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Enemy(EnemyId(7)),
                quantity,
                ReadContext::OutsideTest,
            )
            .total()
        };
        assert_eq!(read(ModifiedQuantity::Fight), 2);
        assert_eq!(read(ModifiedQuantity::Evade), 2);
    }

    // ---- the in-flight test's difficulty --------------------------

    /// A board carrying `test` in flight, one investigator, one location
    /// (printed shroud 2) and one enemy (printed fight 2, evade 2).
    fn state_with_test(basis: crate::state::DifficultyBasis) -> GameState {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .with_location(test_location(3, "Study"))
            .with_enemy(test_enemy(7, "Ghoul"))
            .build();
        state
            .continuations
            .push(crate::state::Continuation::SkillTest(
                crate::state::InFlightSkillTest {
                    difficulty_basis: basis,
                    ..test_skill_test(
                        IN_FLIGHT,
                        InvestigatorId(1),
                        SkillKind::Intellect,
                        SkillTestKind::Investigate,
                        0,
                    )
                },
            ));
        state
    }

    /// Investigator 1's modified `skill`, read under the investigation
    /// [`state_with_test`] puts in flight.
    fn skill_of(state: &GameState, skill: SkillKind) -> i32 {
        modified_value(
            state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(InvestigatorId(1)),
            ModifiedQuantity::Skill(skill),
            ReadContext::DuringTest(SkillTestKind::Investigate),
        )
        .total()
    }

    fn difficulty_of(state: &GameState) -> i32 {
        modified_value(
            state,
            Some(&mock_registry()),
            ModifierTarget::Test,
            ModifiedQuantity::Difficulty,
            ReadContext::DuringTest(SkillTestKind::Investigate),
        )
        .total()
    }

    /// A card-declared difficulty is a printed base value with nothing
    /// behind it on the board.
    #[test]
    fn a_fixed_difficulty_is_its_printed_number() {
        let state = state_with_test(crate::state::DifficultyBasis::Fixed(4));
        assert_eq!(difficulty_of(&state), 4);
    }

    /// An investigation's difficulty *is* the location's modified shroud,
    /// attachments and all.
    #[test]
    fn an_investigations_difficulty_is_the_locations_modified_shroud() {
        let mut state = state_with_test(crate::state::DifficultyBasis::Shroud(LocationId(3)));
        assert_eq!(difficulty_of(&state), 2, "the printed shroud");
        state
            .locations
            .get_mut(&LocationId(3))
            .unwrap()
            .attachments
            .push(CardInPlay::enter_play(
                CardCode::new("shroud-plus-2"),
                CardInstanceId(0),
            ));
        assert_eq!(difficulty_of(&state), 4, "Obscuring Fog's +2, read live");
    }

    /// A Fight's difficulty is the enemy's modified fight value; an
    /// Evade's is its modified evade value.
    #[test]
    fn an_enemys_fight_and_evade_are_the_difficulties_of_attacking_and_evading_it() {
        let state = state_with_test(crate::state::DifficultyBasis::Fight(EnemyId(7)));
        assert_eq!(difficulty_of(&state), 2);
        let mut state = state_with_test(crate::state::DifficultyBasis::Evade(EnemyId(7)));
        assert_eq!(difficulty_of(&state), 2);
        state.enemies.get_mut(&EnemyId(7)).unwrap().evade = 4;
        assert_eq!(difficulty_of(&state), 4, "read live, not banked at ST.1");
    }

    /// The reduction an initiating effect grants (Flashlight 01087's *"-2
    /// shroud for this investigation"*) is a row over the **location**, so
    /// it composes with the location's own modifiers and the clamp lands
    /// once, at the end: 1 + 2 − 2 = 1, not (1 − 2 → 0) + 2 = 2.
    #[test]
    fn a_recorded_row_on_a_location_composes_with_its_attachments() {
        let mut state = state_with_test(crate::state::DifficultyBasis::Shroud(LocationId(3)));
        let loc = state.locations.get_mut(&LocationId(3)).unwrap();
        loc.shroud = 1;
        loc.attachments.push(CardInPlay::enter_play(
            CardCode::new("shroud-plus-2"),
            CardInstanceId(0),
        ));
        state
            .recorded_modifiers
            .push(crate::state::RecordedModifier::targeting(
                ModifierTarget::Location(LocationId(3)),
                InvestigatorId(1),
                Stat::Shroud,
                IntExpr::Lit(-2),
                crate::state::Lifetime::SkillTest(IN_FLIGHT),
                None,
            ));
        assert_eq!(difficulty_of(&state), 1);
    }

    /// The same reduction with nothing else in play goes below zero and is
    /// clamped there — Flashlight's ruling: *"If you reduce shroud to 0,
    /// investigating this location will be successful even if you reveal a
    /// -8 token"* (<https://arkhamdb.com/card/01087>).
    #[test]
    fn a_difficulty_reduced_below_zero_clamps_at_zero() {
        let mut state = state_with_test(crate::state::DifficultyBasis::Shroud(LocationId(3)));
        state.locations.get_mut(&LocationId(3)).unwrap().shroud = 1;
        state
            .recorded_modifiers
            .push(crate::state::RecordedModifier::targeting(
                ModifierTarget::Location(LocationId(3)),
                InvestigatorId(1),
                Stat::Shroud,
                IntExpr::Lit(-2),
                crate::state::Lifetime::SkillTest(IN_FLIGHT),
                None,
            ));
        assert_eq!(difficulty_of(&state), 0);
    }

    /// A row bought for another test contributes nothing to this one's
    /// difficulty either — the identity check is not investigator-specific.
    #[test]
    fn a_location_row_from_another_test_does_not_change_the_difficulty() {
        let mut state = state_with_test(crate::state::DifficultyBasis::Shroud(LocationId(3)));
        state
            .recorded_modifiers
            .push(crate::state::RecordedModifier::targeting(
                ModifierTarget::Location(LocationId(3)),
                InvestigatorId(1),
                Stat::Shroud,
                IntExpr::Lit(-2),
                crate::state::Lifetime::SkillTest(crate::state::SkillTestId(999)),
                None,
            ));
        assert_eq!(difficulty_of(&state), 2, "the printed shroud, unreduced");
    }

    /// Outside a test there is no difficulty to read.
    #[test]
    fn a_difficulty_with_no_test_in_flight_is_zero() {
        let state = GameStateBuilder::new().build();
        let breakdown = modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Test,
            ModifiedQuantity::Difficulty,
            ReadContext::OutsideTest,
        );
        assert_eq!(breakdown.base, 0);
        assert!(breakdown.contributions.is_empty());
    }

    // ---- the elder-sign contribution -----------------------------

    /// `elder_sign_expr` reads the controller's investigator card's
    /// `Trigger::ElderSign { modifier }` and hands back the expression
    /// itself. Roland's `Count(CluesAtControllerLocation)` evaluates to the
    /// clue count at his location; an investigator with no elder-sign
    /// ability has no expression to record at all.
    #[test]
    fn an_elder_sign_modifier_evaluates_its_expression() {
        let mut inv = test_investigator(1);
        inv.investigator_card.code = CardCode::new("elder-sign-clues-here");
        inv.current_location = Some(LocationId(10));
        let mut loc = test_location(10, "Study");
        loc.clues = 2;
        let state = GameStateBuilder::new()
            .with_investigator(inv)
            .with_location(loc)
            .build();
        let expr = elder_sign_expr(&state, &mock_registry(), InvestigatorId(1))
            .expect("the investigator card carries an elder-sign ability");
        let ctx = crate::engine::evaluator::EvalContext::for_controller(InvestigatorId(1));
        assert_eq!(
            crate::engine::evaluator::eval_int_expr(&state, &ctx, &expr),
            Ok(2)
        );

        let mut plain = test_investigator(2);
        plain.investigator_card.code = CardCode::new("no-elder-sign");
        let state = GameStateBuilder::new().with_investigator(plain).build();
        assert_eq!(
            elder_sign_expr(&state, &mock_registry(), InvestigatorId(2)),
            None
        );
    }

    // ---- the fold's stage 5: the test's determination -------------

    /// A determination row scoped to [`IN_FLIGHT`].
    fn determination_row(d: crate::dsl::Determination) -> crate::state::RecordedModifier {
        crate::state::RecordedModifier::determination(
            InvestigatorId(1),
            d,
            crate::state::Lifetime::SkillTest(IN_FLIGHT),
            None,
        )
    }

    /// The board [`state_with_test`] builds (investigator 1 taking an
    /// Intellect investigation against the Study's shroud 2), with `rows`
    /// recorded on top.
    fn state_with_determinations(rows: Vec<crate::state::RecordedModifier>) -> GameState {
        let mut state = state_with_test(crate::state::DifficultyBasis::Shroud(LocationId(3)));
        state.recorded_modifiers = rows;
        state
    }

    /// `glossary/Automatic_Failure_Success.md`: *"If a skill test
    /// automatically fails, the investigator's total skill value for that
    /// test is considered 0."* The difficulty is **not** touched, which is
    /// what keeps the margin real — 0 against shroud 2 fails by 2.
    #[test]
    fn an_automatic_failure_substitutes_the_testers_total_skill_value() {
        let state =
            state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
        assert_eq!(skill_of(&state, SkillKind::Intellect), 0);
        assert_eq!(difficulty_of(&state), 2, "the difficulty is left alone");
    }

    /// *"the investigator's total modified skill value is still
    /// determined, as it may have some bearing on other card abilities"* —
    /// so the substitution replaces the **total**, and the breakdown that
    /// produced it survives intact underneath.
    #[test]
    fn an_automatic_failure_keeps_the_breakdown_it_substitutes() {
        let mut state =
            state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
        state
            .recorded_modifiers
            .push(recorded(InvestigatorId(1), Stat::Intellect, 2));
        let breakdown = modified_value(
            &state,
            Some(&mock_registry()),
            ModifierTarget::Investigator(InvestigatorId(1)),
            ModifiedQuantity::Skill(SkillKind::Intellect),
            ReadContext::DuringTest(SkillTestKind::Investigate),
        );
        assert_eq!(breakdown.base, 3, "the printed intellect");
        assert_eq!(breakdown.contributions.len(), 1, "the +2 row still counts");
        assert_eq!(breakdown.substitution, Some(0));
        assert_eq!(breakdown.total(), 0);
    }

    /// *"If a skill test automatically succeeds, the total difficulty of
    /// that test is considered 0."* The location's own shroud is
    /// unchanged: the substitution lands on the **test's** difficulty, not
    /// on the quantity it is read from.
    #[test]
    fn an_automatic_success_substitutes_the_tests_total_difficulty() {
        let state =
            state_with_determinations(vec![determination_row(Determination::AutomaticSuccess)]);
        assert_eq!(difficulty_of(&state), 0);
        assert_eq!(
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Location(LocationId(3)),
                ModifiedQuantity::Shroud,
                ReadContext::DuringTest(SkillTestKind::Investigate),
            )
            .total(),
            2,
            "the location's shroud is still 2",
        );
        assert_eq!(
            skill_of(&state, SkillKind::Intellect),
            3,
            "the skill value is left alone",
        );
    }

    /// Automatic failure beats automatic success (ADR 0007), and the two
    /// rows coexist: neither suppresses the other, so the answer cannot
    /// depend on which was latched first. Asserted in both orders — the
    /// bug this rules out is a 0-versus-0 comparison passing as a success.
    #[test]
    fn automatic_failure_beats_automatic_success_in_either_latch_order() {
        for rows in [
            vec![
                determination_row(Determination::AutomaticFailure),
                determination_row(Determination::AutomaticSuccess),
            ],
            vec![
                determination_row(Determination::AutomaticSuccess),
                determination_row(Determination::AutomaticFailure),
            ],
        ] {
            let state = state_with_determinations(rows);
            assert_eq!(
                test_determination(&state, ReadContext::DuringTest(SkillTestKind::Investigate)),
                Some(Determination::AutomaticFailure),
            );
            assert_eq!(skill_of(&state, SkillKind::Intellect), 0);
            assert_eq!(
                difficulty_of(&state),
                2,
                "the losing automatic success must not zero the difficulty too,                  or 0 versus 0 compares as a success",
            );
        }
    }

    /// *"the investigator's total skill value **for that test**"* — the
    /// three skills the test is not against are untouched, and so is
    /// anyone else's.
    #[test]
    fn an_automatic_failure_zeroes_only_the_tested_skill() {
        let state =
            state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
        assert_eq!(
            skill_of(&state, SkillKind::Intellect),
            0,
            "the tested skill"
        );
        assert_eq!(skill_of(&state, SkillKind::Willpower), 3);
        assert_eq!(skill_of(&state, SkillKind::Combat), 3);
        assert_eq!(skill_of(&state, SkillKind::Agility), 3);
    }

    /// A determination is a recorded row like any other, so it carries the
    /// skill-test identity check: one bought for another test is inert.
    #[test]
    fn a_determination_scoped_to_another_test_is_inert() {
        let mut state =
            state_with_determinations(vec![crate::state::RecordedModifier::determination(
                InvestigatorId(1),
                Determination::AutomaticFailure,
                crate::state::Lifetime::SkillTest(crate::state::SkillTestId(99)),
                None,
            )]);
        assert_eq!(skill_of(&state, SkillKind::Intellect), 3);
        assert_eq!(
            test_determination(&state, ReadContext::DuringTest(SkillTestKind::Investigate)),
            None,
        );
        // And it is gone for good once its own test tears down.
        state.expire_modifiers_for_test(crate::state::SkillTestId(99));
        assert!(state.recorded_modifiers.is_empty());
    }

    /// A read that declares itself outside a test (prey ranking) sees no
    /// determination, exactly as it sees no recorded row.
    #[test]
    fn a_read_outside_a_test_sees_no_determination() {
        let state =
            state_with_determinations(vec![determination_row(Determination::AutomaticFailure)]);
        assert_eq!(test_determination(&state, ReadContext::OutsideTest), None,);
        assert_eq!(
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Investigator(InvestigatorId(1)),
                ModifiedQuantity::Skill(SkillKind::Intellect),
                ReadContext::OutsideTest,
            )
            .total(),
            3,
        );
    }
}
