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
//! keep in sync. Seven collections carry them:
//!
//! 1. every investigator's controlled instances (investigator card,
//!    cards in play, threat area),
//! 2. every location's own card,
//! 3. every location's attachments,
//! 4. every location's cards put into play *at* it, controlled by
//!    nobody (Lita Chantler 01117 in the Parlor),
//! 5. every enemy's own card,
//! 6. every enemy's attachments,
//! 7. the current act and the current agenda.
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
//! [determination](card_dsl::dsl::Determination) in place of a delta — the
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

use card_dsl::card_data::SkillKind;
use card_dsl::dsl::{
    Determination, Effect, IntExpr, ModifierAudience, ModifierScope, SkillTestKind, Stat, Trigger,
};

use crate::card_registry::CardRegistry;
use crate::engine::abilities_in_effect;
use crate::engine::evaluator::{self, EvalContext};
use crate::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, DifficultyBasis, EnemyId, GameState,
    InvestigatorId, LocationId, RecordedModifierKind,
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
        instance: Option<CardInstanceId>,
    },
    /// A recorded row queued by an earlier effect resolution, attributed
    /// to the in-play instance that pushed it where one is known.
    Recorded {
        /// The instance whose ability pushed the row.
        instance: Option<CardInstanceId>,
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
    /// The contributions are **kept**, not discarded, so a substituted
    /// total still has a composition underneath it. The glossary states
    /// that requirement for an automatic *success* — its worked example
    /// has Patrice skipping the reveal, and *"the skill test still takes
    /// place. Cards may still be committed to the test, and the
    /// investigator's total modified skill value is still determined, as
    /// it may have some bearing on other card abilities"* — and says
    /// nothing either way about an automatic failure. Keeping the
    /// breakdown on both reads the same rule the same way rather than
    /// discarding information on the case the example does not cover.
    pub substitution: Option<i32>,
}

impl ModifierBreakdown {
    /// A breakdown that is nothing but its base value: no contribution can
    /// reach it and no determination substitutes it. The shape of a
    /// printed difficulty, and of a read with no test in flight.
    #[must_use]
    fn flat(base: i32) -> Self {
        Self {
            base,
            contributions: Vec::new(),
            substitution: None,
        }
    }

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
            let mut breakdown = ModifierBreakdown::flat(base_value(state, target, quantity));
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
/// A basis naming an entity that has **left play** reads the same way — the
/// delegate finds nothing and answers a base of 0 — but no verdict is ever
/// taken against that 0: `skill_test::advance` abandons a test whose
/// difficulty target has left play before it reaches ST.6 ([`DifficultyBasis`]
/// records the decision and why the vendored sources do not settle it, #682).
/// So the absent-target read survives only as a display value for a client
/// asking about a test that is on its way out.
fn difficulty(
    state: &GameState,
    registry: Option<&CardRegistry>,
    context: ReadContext,
) -> ModifierBreakdown {
    let Some(basis) = state.current_skill_test().map(|t| t.difficulty_basis) else {
        return ModifierBreakdown::flat(0);
    };
    let (target, quantity) = match basis {
        DifficultyBasis::Fixed(n) => return ModifierBreakdown::flat(i32::from(n)),
        DifficultyBasis::Shroud(id) => (ModifierTarget::Location(id), ModifiedQuantity::Shroud),
        DifficultyBasis::Fight(id) => (ModifierTarget::Enemy(id), ModifiedQuantity::Fight),
        DifficultyBasis::Evade(id) => (ModifierTarget::Enemy(id), ModifiedQuantity::Evade),
    };
    modified_value(state, registry, target, quantity, context)
}

/// Stage 1: the printed value. Base replacement (Duke 02014) lands with
/// that card; until then the base is always what the card prints.
///
/// A replaced base is still only stage 1 — stages 2–4 stack on top of it,
/// they do not fall away. `data/official-faq/Frequently_Asked_Questions.md`,
/// on George Barnaby 11017's ability setting a base maximum hand size: *"While
/// George Barnaby's ability sets his **base** maximum hand size to be equal to
/// the number of facedown cards beneath him (rather than the default value of
/// 8), **this value can then be modified by other abilities**."* So a stage-1
/// replacement must return here rather than short-circuit the fold — which is
/// what #673 will build Duke against.
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
    /// A card put into play **at** a location, controlled by nobody
    /// (Lita Chantler 01117 in the Parlor). Its location is that
    /// location, so a location-scoped audience resolves from there —
    /// but it is not an attachment, so
    /// [`ModifierAudience::AttachedCard`] does not reach through it.
    AtLocation(LocationId),
    /// An enemy's own card.
    Enemy(EnemyId),
    /// A card attached to an enemy.
    EnemyAttachment(EnemyId),
    /// The current act or agenda. Has no location of its own, so
    /// location-scoped audiences never resolve from one.
    ActAgenda,
}

/// Sweep the seven collections, pushing every active modifier that
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
///
/// # A granted modifier is a modifier (#773)
///
/// The abilities a card sits behind here come from
/// [`abilities_in_effect::for_source`], the same funnel every *ability* path
/// uses — so they are the printed ones on the side in effect **plus** whatever
/// the board grants (ADR 0014). Lita Chantler 01117's *"Each investigator at
/// your location gets +1 \[combat\]"* is granted to her by her own card while
/// a player controls her, and a sweep reading `abilities_for` directly would
/// have found her printing nothing and contributed nothing.
///
/// Two consequences of routing through it, both deliberate:
///
/// - **The walk is quadratic.** `for_source` runs its own board walk per
///   source to find that source's granters, inside this walk over every
///   source. The corpus is small enough (a board is tens of cards, and every
///   read is already a full re-derivation) that measuring it would cost more
///   than it saves; if it ever matters, the fix is to sweep grants once per
///   read rather than once per source.
/// - **A grant's condition is evaluated during a modifier read.** `granted_to`
///   answers a non-board-global condition through `eval_condition`, which could
///   in principle ask for a modified value and re-enter this sweep. It does not
///   today: both corpus grants — Lita's and the Parlor 01115's — condition on
///   `Condition::ControlStatus`, which is board-global and reads no modified
///   quantity. A future grant conditioned on a *stat* is the case that would
///   need a depth guard.
fn sweep(
    state: &GameState,
    registry: &CardRegistry,
    target: ModifierTarget,
    quantity: ModifiedQuantity,
    context: ReadContext,
    out: &mut Vec<Contribution>,
) {
    let mut visit = |code: &CardCode,
                     instance: Option<&CardInPlay>,
                     placement: Placement,
                     source: AbilitySource| {
        // The funnel: the side in effect — a location's back's constants while
        // unrevealed, its front's while revealed (#774) — plus whatever the
        // board grants this card (#772). The address each ability comes paired
        // with names it across a suspension; a modifier is read fresh at every
        // read and never suspended, so it is dropped here.
        let Some(abilities) = abilities_in_effect::for_source_with(state, registry, source, code)
        else {
            return;
        };
        for (_, ability) in &abilities {
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
            visit(
                &card.code,
                Some(card),
                Placement::Controlled(inv.id),
                AbilitySource::InPlay(card.instance_id),
            );
        }
    }
    // 2, 3 and 4. Every location, its attachments, and the cards put into
    //    play at it.
    for (id, loc) in &state.locations {
        visit(
            &loc.code,
            None,
            Placement::Location(*id),
            AbilitySource::Location(*id),
        );
        for att in &loc.attachments {
            visit(
                &att.code,
                Some(att),
                Placement::LocationAttachment(*id),
                AbilitySource::InPlay(att.instance_id),
            );
        }
        for card in &loc.cards_at_location {
            visit(
                &card.code,
                Some(card),
                Placement::AtLocation(*id),
                AbilitySource::InPlay(card.instance_id),
            );
        }
    }
    // 5 and 6. Every enemy and its attachments.
    for (id, enemy) in &state.enemies {
        visit(
            &enemy.code,
            None,
            Placement::Enemy(*id),
            AbilitySource::Enemy(*id),
        );
        for att in &enemy.attachments {
            visit(
                &att.code,
                Some(att),
                Placement::EnemyAttachment(*id),
                AbilitySource::InPlay(att.instance_id),
            );
        }
    }
    // 7. The current act and agenda. `Placement::ActAgenda` is one value
    //    because neither is anywhere on the board, but the *ability source* is
    //    two: a grant addressed to the act must not be found on the agenda.
    if let Some(act) = state.act_deck.get(state.act_index) {
        visit(&act.code, None, Placement::ActAgenda, AbilitySource::Act);
    }
    if let Some(agenda) = state.agenda_deck.get(state.agenda_index) {
        visit(
            &agenda.code,
            None,
            Placement::ActAgenda,
            AbilitySource::Agenda,
        );
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
/// [`IntExpr::Lit`](card_dsl::dsl::IntExpr::Lit), as does the revealed chaos
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
        // `AbilitySource::InPlay` is an **address** — *"any card instance in
        // play, wherever it sits"* — not a record that an ability was
        // activated, so a recorded row's origin instance is one honestly. The
        // row itself stays instance-valued (#834): widening it would push the
        // same narrowing one layer down onto `ContributionSource::Recorded`.
        let eval_ctx = EvalContext::for_controller_with_optional_source(
            row.investigator,
            row.source.map(AbilitySource::InPlay),
        );
        let Ok(delta) = evaluator::eval_int_expr(state, &eval_ctx, delta) else {
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
/// controlled card, the location itself for a location, its attachments
/// or a card put into play at it, the enemy's for an enemy or its
/// attachments. The act and agenda are nowhere, and so reach no
/// location-scoped audience.
fn source_location(state: &GameState, placement: Placement) -> Option<LocationId> {
    match placement {
        Placement::Controlled(id) => state
            .investigators
            .get(&id)
            .and_then(|inv| inv.current_location),
        Placement::Location(id) | Placement::LocationAttachment(id) | Placement::AtLocation(id) => {
            Some(id)
        }
        Placement::Enemy(id) | Placement::EnemyAttachment(id) => state
            .enemies
            .get(&id)
            .and_then(|enemy| enemy.current_location),
        Placement::ActAgenda => None,
    }
}

/// The controller's **elder-sign** skill-test modifier as an
/// [`IntExpr`](card_dsl::dsl::IntExpr): the expression on their investigator
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
) -> Option<IntExpr> {
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
mod tests;
