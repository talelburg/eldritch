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
//! *recorded* instead of derived; today that is
//! [`PendingSkillModifier`], the
//! [`ModifierScope::ThisSkillTest`] row an activated ability pushes.
//!
//! # The fold
//!
//! ADR 0005 folds a modified quantity in the Rules Reference's own order
//! — base value, a transform over rows, addition and subtraction,
//! doubling and halving with rounding, then the clamp and whole-quantity
//! substitution. This module implements the base, the additive pass and
//! the clamp; the row transform and the multiplicative pass are #676's,
//! and substitution (automatic failure and success) is #677's. Because
//! the clamp is last, [`ModifierBreakdown`] exposes the unclamped sum as
//! well: a caller that still has modifiers of its own to add — the
//! skill-test handler, which adds the revealed token's ±N after this
//! query returns — must add them *before* clamping, or `Modifiers.md`'s
//! own worked example comes out wrong (base 4, a −8 token and a +2 is
//! −2 → 0, **not** 0 + 2 → 2).

use crate::card_data::SkillKind;
use crate::card_registry::CardRegistry;
use crate::dsl::{Effect, ModifierAudience, ModifierScope, SkillTestKind, Stat, Trigger};
use crate::state::{
    CardCode, CardInPlay, EnemyId, GameState, InvestigatorId, LocationId, PendingSkillModifier,
};

/// Which entity's quantity is being asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierTarget {
    /// An investigator's skills and capacities.
    Investigator(InvestigatorId),
    /// A location's shroud.
    Location(LocationId),
    /// An enemy's fight, evade or health.
    Enemy(EnemyId),
    /// The in-flight skill test itself — its difficulty. Nothing
    /// contributes to it yet: the difficulty is still snapshotted at
    /// initiation, and re-homing it onto the tested location or enemy is
    /// #677's. Reading it through this query is what gives that slice a
    /// single place to change.
    Test,
}

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
    /// The in-flight skill test's difficulty.
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
    /// which is #676's stage 1 and no card declares yet).
    pub base: i32,
    /// Every active modifier, in sweep order.
    pub contributions: Vec<Contribution>,
}

impl ModifierBreakdown {
    /// The fold's result **before** the clamp: base plus every
    /// contribution. Callers with modifiers still to add (the revealed
    /// chaos token's ±N) sum against this and clamp themselves, since
    /// `Modifiers.md` puts the clamp after *all* modifiers.
    #[must_use]
    pub fn raw_total(&self) -> i32 {
        self.contributions
            .iter()
            .fold(self.base, |acc, c| acc.saturating_add(i32::from(c.delta)))
    }

    /// The modified value: [`raw_total`](Self::raw_total) with the
    /// clamp applied. `Modifiers.md`: *"after all active modifiers have
    /// been applied, any resultant value below zero is treated as
    /// zero"*.
    #[must_use]
    pub fn total(&self) -> i32 {
        self.raw_total().max(0)
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
    let mut breakdown = ModifierBreakdown {
        base: base_value(state, target, quantity),
        contributions: Vec::new(),
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
}

/// Stage 1: the printed value. Base replacement (Duke 02014) is #676's;
/// until it lands the base is always what the card prints.
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
        (ModifierTarget::Test, ModifiedQuantity::Difficulty) => state
            .current_skill_test()
            .map_or(0, |t| i32::from(t.difficulty)),
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
/// Only [`PendingSkillModifier`] exists today, and only inside a test —
/// its scope is [`ModifierScope::ThisSkillTest`], so a read outside a
/// test (prey ranking) must not see it. Giving those rows a test
/// identity, so a row from a *previous* test is inert rather than
/// merely drained, is #676's.
fn collect_recorded(
    state: &GameState,
    target: ModifierTarget,
    quantity: ModifiedQuantity,
    context: ReadContext,
    out: &mut Vec<Contribution>,
) {
    let (ModifierTarget::Investigator(id), ReadContext::DuringTest(_)) = (target, context) else {
        return;
    };
    for PendingSkillModifier {
        investigator,
        stat,
        delta,
        source,
        ..
    } in &state.pending_skill_modifiers
    {
        if *investigator == id && stat_matches(*stat, quantity) {
            out.push(Contribution {
                source: ContributionSource::Recorded { instance: *source },
                delta: *delta,
            });
        }
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
        ModifiedQuantity::Skill(SkillKind::Willpower) => stat == Stat::Willpower,
        ModifiedQuantity::Skill(SkillKind::Intellect) => stat == Stat::Intellect,
        ModifiedQuantity::Skill(SkillKind::Combat) => stat == Stat::Combat,
        ModifiedQuantity::Skill(SkillKind::Agility) => stat == Stat::Agility,
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

/// The controller's **elder-sign** skill-test modifier: the
/// [`IntExpr`](crate::dsl::IntExpr) on their investigator card's
/// [`Trigger::ElderSign`] ability, evaluated for the controller.
///
/// Lives here rather than in the evaluator because it answers the same
/// question as [`modified_value`] — what is contributing to this
/// investigator's total right now — for the one contributor the sweep
/// cannot see: the revealed chaos token. It is not folded into the
/// breakdown yet because the token's own ±N is still resolved to an
/// integer at ST.3 and added by the skill-test handler; both become
/// recorded rows in #676, and this function disappears into the sweep
/// with them.
///
/// Returns `0` when the controller is not found, the card isn't in the
/// registry, or it carries no elder-sign ability — so every investigator
/// without an elder-sign resolves as 0.
///
/// **Scope (#118), sunset by #448:** handles only pure-modifier
/// elder-signs. Signs that also run an effect (Daisy / Agnes) are
/// deferred — see [`Trigger::ElderSign`].
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
    let ctx = super::evaluator::EvalContext::for_controller(controller);
    for ability in &abilities {
        if let Trigger::ElderSign { modifier } = &ability.trigger {
            // A malformed elder-sign IntExpr (unexpressible Condition) yields
            // Err; treat it as no bonus rather than panicking mid-test — the
            // only in-scope IntExpr is Count(CluesAtControllerLocation), which
            // is always Ok.
            return super::evaluator::eval_int_expr(state, &ctx, modifier).unwrap_or(0);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsl::{constant, elder_sign, modify, modify_for, on_play, Ability, IntExpr};
    use crate::state::{CardInstanceId, LocationId};
    use crate::test_support::{test_enemy, test_investigator, test_location, GameStateBuilder};

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
        .raw_total()
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
        };
        assert_eq!(breakdown.raw_total(), -2, "the fold itself does not clamp");
        assert_eq!(breakdown.total(), 0, "4 - 8 + 2 = -2, treated as 0");
        assert_ne!(
            breakdown.total(),
            2,
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
            .raw_total()
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
                .raw_total(),
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
            .raw_total(),
            3,
        );
    }

    // ---- recorded rows -------------------------------------------

    fn state_with_pending(pending: Vec<PendingSkillModifier>) -> GameState {
        let mut state = GameStateBuilder::new()
            .with_investigator(test_investigator(1))
            .build();
        state.pending_skill_modifiers = pending;
        state
    }

    fn pending(investigator: InvestigatorId, stat: Stat, delta: i8) -> PendingSkillModifier {
        PendingSkillModifier {
            investigator,
            stat,
            delta,
            source: None,
        }
    }

    #[test]
    fn recorded_rows_for_the_target_are_summed() {
        let id = InvestigatorId(1);
        let state = state_with_pending(vec![
            pending(id, Stat::Intellect, 1),
            pending(id, Stat::Intellect, 2),
        ]);
        assert_eq!(skill(&state, id, SkillKind::Intellect), 6);
    }

    #[test]
    fn a_recorded_row_for_another_investigator_is_ignored() {
        let state = state_with_pending(vec![pending(InvestigatorId(2), Stat::Willpower, 5)]);
        assert_eq!(skill(&state, InvestigatorId(1), SkillKind::Willpower), 3);
    }

    #[test]
    fn a_recorded_row_for_another_stat_is_ignored() {
        let id = InvestigatorId(1);
        let state = state_with_pending(vec![
            pending(id, Stat::Intellect, 1),
            pending(id, Stat::MaxHealth, 1),
        ]);
        assert_eq!(skill(&state, id, SkillKind::Willpower), 3);
    }

    /// A `ThisSkillTest` row is scoped to a test, so a read that isn't
    /// inside one — prey ranking — must not see it.
    #[test]
    fn a_recorded_row_is_invisible_outside_a_test() {
        let id = InvestigatorId(1);
        let state = state_with_pending(vec![pending(id, Stat::Willpower, 5)]);
        assert_eq!(
            modified_value(
                &state,
                Some(&mock_registry()),
                ModifierTarget::Investigator(id),
                ModifiedQuantity::Skill(SkillKind::Willpower),
                ReadContext::OutsideTest,
            )
            .raw_total(),
            3,
        );
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

    // ---- the elder-sign contribution -----------------------------

    /// `elder_sign_modifier` reads the controller's investigator card's
    /// `Trigger::ElderSign { modifier }` and evaluates it. Roland's
    /// `Count(CluesAtControllerLocation)` returns the clue count at his
    /// location; an investigator with no elder-sign ability returns 0.
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
        assert_eq!(
            elder_sign_modifier(&state, &mock_registry(), InvestigatorId(1)),
            2
        );

        let mut plain = test_investigator(2);
        plain.investigator_card.code = CardCode::new("no-elder-sign");
        let state = GameStateBuilder::new().with_investigator(plain).build();
        assert_eq!(
            elder_sign_modifier(&state, &mock_registry(), InvestigatorId(2)),
            0
        );
    }
}
