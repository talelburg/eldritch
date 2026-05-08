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
/// Phase-2 minimal set. Later phases add `OnEvent(EventPattern)`,
/// `AtPhaseStart`/`AtPhaseEnd`, `DuringSkillTest`, `OnLeavePlay`,
/// `Activated { action_cost: u8, ... }` (covering both `[action]` and
/// `[fast]` activated abilities), and reaction triggers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}

// ---- abilities -------------------------------------------------

/// One ability on a card: a trigger paired with the effect that
/// resolves when the trigger fires.
///
/// A card may have multiple [`Ability`] entries — e.g. a constant
/// modifier plus a forced-on-leave-play effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Ability {
    pub trigger: Trigger,
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
/// Phase-2 minimal set. Mid-test buffs would need `ThisSkillTest`;
/// turn-scoped ones use `ThisTurn`. The most common scope by far is
/// `WhileInPlay` (constant modifiers from assets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ModifierScope {
    /// Active for as long as the source card is in play. Used by
    /// constant abilities (Holy Rosary, Magnifying Glass).
    WhileInPlay,
    /// Active until the current skill test resolves. Used by
    /// commit-time bonuses and action abilities like Hyperawareness.
    ThisSkillTest,
    /// Active until the end of the current investigator turn.
    ThisTurn,
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
    Controllers,
    /// The controller picks a location.
    ChosenByController,
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
    /// True if the most recent skill test (in the current resolution
    /// stack) was a success. Used by Deduction-shaped effects.
    SkillTestSucceeded,
}

// ---- builders --------------------------------------------------

/// Construct a [`Trigger::Constant`]-driven [`Ability`] wrapping the
/// given effect.
#[must_use]
pub fn constant(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::Constant,
        effect,
    }
}

/// Construct a [`Trigger::OnPlay`]-driven [`Ability`] wrapping the
/// given effect.
#[must_use]
pub fn on_play(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::OnPlay,
        effect,
    }
}

/// Construct a [`Trigger::OnCommit`]-driven [`Ability`] wrapping the
/// given effect. Used by skill cards and other commit-trigger cards.
#[must_use]
pub fn on_commit(effect: Effect) -> Ability {
    Ability {
        trigger: Trigger::OnCommit,
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

    /// Holy Rosary's full shape: two constant modifiers (willpower
    /// and max-sanity), naturally expressed as two separate Ability
    /// declarations on the card.
    #[test]
    fn holy_rosary_full_shape_compiles_as_two_abilities() {
        let abilities = [
            constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay)),
            constant(modify(Stat::MaxSanity, 2, ModifierScope::WhileInPlay)),
        ];
        assert_eq!(abilities.len(), 2);
    }

    /// Working a Hunch's "fast event: discover 1 clue at your location"
    /// — the canonical `OnPlay` + `DiscoverClue` shape.
    #[test]
    fn working_a_hunch_compiles() {
        let ability = on_play(discover_clue(LocationTarget::Controllers, 1));
        assert_eq!(ability.trigger, Trigger::OnPlay);
        assert!(matches!(
            ability.effect,
            Effect::DiscoverClue {
                from: LocationTarget::Controllers,
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
            Condition::SkillTestSucceeded,
            discover_clue(LocationTarget::Controllers, 1),
        ));
        assert_eq!(ability.trigger, Trigger::OnCommit);
        // Distinct enum variant — compiler enforces the difference at
        // every match site, which is the whole point of separating
        // them rather than reusing OnPlay.
        assert_ne!(ability.trigger, Trigger::OnPlay);
    }

    /// Sequence composition: a hypothetical "gain 1 resource AND
    /// discover 1 clue at your location" combined effect.
    #[test]
    fn seq_composition_nests_two_effects() {
        let effect = seq([
            gain_resources(InvestigatorTarget::Controller, 1),
            discover_clue(LocationTarget::Controllers, 1),
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
            Condition::SkillTestSucceeded,
            discover_clue(LocationTarget::Controllers, 1),
        );
        let with_else = if_else(
            Condition::SkillTestSucceeded,
            discover_clue(LocationTarget::Controllers, 1),
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
            discover_clue(LocationTarget::Controllers, 1),
        ]);
        match effect {
            Effect::ChooseOne(alts) => assert_eq!(alts.len(), 2),
            _ => panic!("expected ChooseOne"),
        }
    }

    /// Effects clone deeply (the recursive Box doesn't break Clone).
    #[test]
    fn deeply_nested_effect_clones() {
        let original = seq([
            if_else(
                Condition::SkillTestSucceeded,
                for_each(
                    InvestigatorTargetSet::AtControllerLocation,
                    gain_resources(InvestigatorTarget::Active, 1),
                ),
                modify(Stat::Intellect, -1, ModifierScope::ThisSkillTest),
            ),
            choose_one([
                discover_clue(LocationTarget::Controllers, 1),
                gain_resources(InvestigatorTarget::Controller, 2),
            ]),
        ]);
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }
}
