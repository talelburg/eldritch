//! The card registry: the engine ↔ cards crate lookup bridge.
//!
//! The engine (in `game-core`) needs to look up a card's metadata
//! (type, class, traits, …) and abilities ([`Vec<Ability>`]) by
//! [`CardCode`] when resolving actions like `PlayCard` or computing
//! constant-modifier contributions during a skill test. The card
//! corpus and ability implementations live in the `cards` crate, which
//! depends on `game-core` — so a direct call from `game-core` into
//! `cards` is impossible.
//!
//! This module bridges the gap with a value. The `cards` crate exposes a
//! [`CardRegistry`] (function pointers into its own `by_code` /
//! `abilities_for`), and the host hands it to every
//! [`apply`](crate::engine::apply) inside a
//! [`Registries`](crate::engine::Registries); the engine carries it on its
//! context for the length of the apply. There is no process global, so "no
//! registry" cannot happen and each test chooses the registry it runs
//! against.
//!
//! # Why function pointers, not `dyn Trait`?
//!
//! The lookup interface is small and fixed. A plain struct of `fn`
//! pointers avoids vtable overhead, makes the registry [`Copy`], and
//! keeps `dyn` out of the engine's hot path. Tests construct registry
//! values with custom function pointers to mock the corpus when
//! needed.

use card_dsl::card_data::CardMetadata;
use card_dsl::dsl::{Ability, CardFilter};

use crate::engine::evaluator::EvalContext;
use crate::engine::{EngineOutcome, NativeCx};
use crate::state::{CardCode, GameState};

/// A card-local Rust effect: reads the state and changes the game through the
/// engine exits on [`NativeCx`], returning the resolution outcome. Provided by
/// the `cards` crate and dispatched from [`Effect::Native`] via
/// [`CardRegistry::native_effect_for`].
///
/// [`Effect::Native`]: card_dsl::dsl::Effect::Native
pub type NativeEffectFn = fn(&mut NativeCx, &EvalContext) -> EngineOutcome;

/// A card-local read-only predicate backing a [`Condition::Native`] gate:
/// returns whether the condition holds against the current board and evaluation
/// context. Dispatched from `eval_condition` via
/// [`CardRegistry::native_condition_for`], whether the condition guards an
/// effect or an ability's
/// [`eligibility`](card_dsl::dsl::Ability::eligibility).
///
/// [`Condition::Native`]: card_dsl::dsl::Condition::Native
pub type NativeConditionFn = fn(&GameState, &EvalContext) -> bool;

/// Bundle of card-lookup function pointers.
///
/// The `cards` crate provides a static instance, `cards::REGISTRY`, whose
/// slots search its per-card registration records; tests can construct ad-hoc
/// instances with mock function pointers.
#[derive(Debug, Clone, Copy)]
pub struct CardRegistry {
    /// Look up static metadata by code. Returns `None` for unknown
    /// codes.
    pub metadata_for: fn(&CardCode) -> Option<&'static CardMetadata>,
    /// Look up hand-implemented abilities by code. Returns `None` for
    /// unimplemented (or unknown) cards.
    pub abilities_for: fn(&CardCode) -> Option<Vec<Ability>>,
    /// Look up the abilities printed on a card's **reverse side** by code.
    /// Returns `None` for a card with no implemented back-side abilities —
    /// which is every card but the Parlor 01115 today.
    ///
    /// A location's back is in effect exactly while the location is
    /// **unrevealed**, and its front exactly while it is revealed (#774), so
    /// nothing reads this slot directly: engine code goes through
    /// `engine::abilities_in_effect`, which owns
    /// that choice. Reading `abilities_for` for a location instead is the bug
    /// this slot exists to make impossible to write by accident — an
    /// unrevealed Parlor would offer its front's **Resign**, and would not
    /// carry its back's barrier.
    ///
    /// It is deliberately a second slot rather than a `side` tag on
    /// [`Ability`]: the side is a property of where the text is printed, not
    /// of the ability, and every non-location card in the corpus would carry
    /// a field that is always `Front`.
    pub back_abilities_for: fn(&CardCode) -> Option<Vec<Ability>>,
    /// Look up a card-local Rust effect by its [`Effect::Native`] tag.
    /// Returns `None` for unregistered tags.
    ///
    /// [`Effect::Native`]: card_dsl::dsl::Effect::Native
    pub native_effect_for: fn(&str) -> Option<NativeEffectFn>,
    /// Look up a card-local condition predicate by its [`Condition::Native`]
    /// tag. Returns `None` for unregistered tags.
    ///
    /// The registry's **one** read-only native slot: every card predicate the
    /// declarative vocabulary can't express resolves here, whether it guards an
    /// effect or an ability's
    /// [`eligibility`](card_dsl::dsl::Ability::eligibility). See
    /// [`Condition::Native`] for its consumers and `TODO(#609)`, the promotion
    /// that retires the compound, target-referencing ones.
    ///
    /// [`Condition::Native`]: card_dsl::dsl::Condition::Native
    pub native_condition_for: fn(&str) -> Option<NativeConditionFn>,
    /// Whether a card has an implementation and may be taken into a scenario.
    /// Scenario start refuses a roster whose investigator or deck holds a code
    /// this rejects, naming every such code, so an unimplemented card is never
    /// seated to sit in play doing nothing.
    ///
    /// The production registry wires this to `cards::is_playable`. Test
    /// registries accept every code, so synthetic test decks seat.
    pub is_playable: fn(&CardCode) -> bool,
}

impl CardRegistry {
    /// A registry that knows nothing: every lookup returns `None`.
    /// It also refuses nothing: [`is_playable`](Self::is_playable) accepts
    /// every code, so a deck of synthetic codes seats under a registry built
    /// from it.
    ///
    /// **The base every partial registry is built from**, so a literal names
    /// only the slots it actually implements:
    ///
    /// ```
    /// # use game_core::card_registry::CardRegistry;
    /// # use game_core::state::CardCode;
    /// # use card_dsl::dsl::Ability;
    /// # fn mock_abilities_for(_: &CardCode) -> Option<Vec<Ability>> { None }
    /// let reg = CardRegistry {
    ///     abilities_for: mock_abilities_for,
    ///     ..CardRegistry::EMPTY
    /// };
    /// ```
    ///
    /// This exists because adding a slot used to mean editing **every literal
    /// in the workspace**, each gaining one more `|_| None,`.
    /// That is churn no reviewer can read, and it buries the one or two
    /// literals where the new slot actually matters. With `EMPTY` as the base,
    /// a new slot touches only the literals that implement it.
    ///
    /// `const`, so a `const` registry (`cards::REGISTRY`, or a test binary's
    /// own probe registry) can spread it too.
    ///
    /// **Spread `EMPTY` only over a registry you are building from nothing.**
    /// A helper wrapping a *real* registry must spread that registry instead —
    /// naming a slot it doesn't override silently switches the real lookup off,
    /// which is exactly the bug #774's review caught in the helper that
    /// composed the synthetic test cards into a real registry.
    pub const EMPTY: Self = Self {
        metadata_for: |_| None,
        abilities_for: |_| None,
        back_abilities_for: |_| None,
        native_effect_for: |_| None,
        native_condition_for: |_| None,
        is_playable: |_| true,
    };
}

impl Default for CardRegistry {
    /// [`CardRegistry::EMPTY`] — the knows-nothing registry.
    fn default() -> Self {
        Self::EMPTY
    }
}

/// Whether the printed card with `code` matches `filter`
/// ([`CardFilter::matches`] on its registry metadata).
///
/// This is game-core's only registry-backed card-filter check; anything in
/// the engine that asks "is this card a Tome asset?" or "is this enemy
/// Elite?" asks it here, keyed by code, so the kernel never names a trait.
///
/// Returns `false` when `cards` has no metadata for `code`. A filtered deck
/// search then finds nothing rather than panicking.
#[must_use]
pub fn card_matches(cards: &CardRegistry, code: &CardCode, filter: &CardFilter) -> bool {
    (cards.metadata_for)(code).is_some_and(|meta| filter.matches(meta))
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use card_dsl::card_data::{CardKind, CardType, Class, SkillIcons};
    use card_dsl::dsl::{self, ModifierScope, Stat};

    use super::*;

    /// Build a hand-rolled `CardMetadata` for a fake test card.
    fn fake_metadata() -> CardMetadata {
        CardMetadata {
            code: "TEST1".to_string(),
            name: "Test Card".to_string(),
            text: None,
            traits: vec![],
            back_name: None,
            back_text: None,
            pack_code: "test".to_string(),
            weakness: false,
            kind: CardKind::Asset {
                class: Class::Neutral,
                cost: Some(0),
                xp: Some(0),
                slots: vec![],
                health: None,
                sanity: None,
                skill_icons: SkillIcons::default(),
                is_fast: false,
                deck_limit: 2,
                uses: None,
                play_only_during_turn: false,
            },
        }
    }

    fn fake_metadata_static() -> &'static CardMetadata {
        static M: OnceLock<CardMetadata> = OnceLock::new();
        M.get_or_init(fake_metadata)
    }

    fn fake_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
        if code.as_str() == "TEST1" {
            Some(fake_metadata_static())
        } else {
            None
        }
    }

    fn fake_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
        if code.as_str() == "TEST1" {
            Some(vec![dsl::constant(dsl::modify(
                Stat::Willpower,
                1,
                ModifierScope::WhileInPlay,
            ))])
        } else {
            None
        }
    }

    /// A locally-constructed registry exercising both function-pointer
    /// fields.
    fn fake_registry() -> CardRegistry {
        CardRegistry {
            metadata_for: fake_metadata_for,
            abilities_for: fake_abilities_for,
            ..CardRegistry::EMPTY
        }
    }

    #[test]
    fn metadata_lookup_returns_known_card() {
        let reg = fake_registry();
        let code = CardCode::new("TEST1");
        let meta = (reg.metadata_for)(&code).expect("known code should resolve");
        assert_eq!(meta.code, "TEST1");
        assert_eq!(meta.card_type(), CardType::Asset);
    }

    #[test]
    fn metadata_lookup_returns_none_for_unknown_code() {
        let reg = fake_registry();
        let code = CardCode::new("99999");
        assert!((reg.metadata_for)(&code).is_none());
    }

    #[test]
    fn abilities_lookup_returns_known_card() {
        let reg = fake_registry();
        let code = CardCode::new("TEST1");
        let abilities = (reg.abilities_for)(&code).expect("known code should resolve");
        assert_eq!(abilities.len(), 1);
    }

    #[test]
    fn abilities_lookup_returns_none_for_unknown_code() {
        let reg = fake_registry();
        let code = CardCode::new("99999");
        assert!((reg.abilities_for)(&code).is_none());
    }
}
