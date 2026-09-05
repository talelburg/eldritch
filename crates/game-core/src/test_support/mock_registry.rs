//! A builder for the mock [`CardRegistry`] a test binary installs.
//!
//! Integration binaries across `game-core` and `cards` hand-roll the same
//! plumbing: a `OnceLock<CardMetadata>` per probe card so the lookup can hand
//! back a `&'static`, an `abilities_for` match over a handful of invented codes,
//! and a constructor-time install of a `CardRegistry` literal spread over
//! [`CardRegistry::EMPTY`]. [`MockRegistry`] collapses that plumbing.
//!
//! ```
//! use game_core::dsl::{constant, modify, ModifierScope, Stat};
//! use game_core::test_support::MockRegistry;
//!
//! MockRegistry::new()
//!     .with_abilities("_doc_probe", || {
//!         vec![constant(modify(Stat::Willpower, 1, ModifierScope::WhileInPlay))]
//!     })
//!     .install();
//! ```
//!
//! A card's metadata rides [`with_card`](MockRegistry::with_card), which takes a
//! `CardMetadata` the caller builds — there is no shared constructor for one,
//! deliberately.
//!
//! **It collapses plumbing only.** Per [ADR 0016] a synthetic fixture models an
//! engine primitive and never impersonates a printed card, and probe cards stay
//! test-local — one per reader, defined in the file that reads them. So this
//! module exposes no named probe cards and no library of them: every code and
//! every `CardMetadata` here comes from the caller. A shared probe library would
//! recreate, in a new location, the shared-fixture problem that the test-substrate
//! migration ([#864]) exists to undo.
//!
//! [ADR 0016]: https://github.com/talelburg/eldritch/blob/main/docs/adr/0016-a-synthetic-fixture-models-a-primitive-never-a-printed-card.md
//! [#864]: https://github.com/talelburg/eldritch/issues/864

use std::sync::OnceLock;

use crate::card_data::CardMetadata;
use crate::card_registry::{self, CardRegistry, EligibilityFn, NativeConditionFn, NativeEffectFn};
use crate::dsl::Ability;
use crate::state::CardCode;

/// A per-code abilities producer. Boxed rather than a `fn` pointer so a caller
/// can close over locals — a `Vec<Ability>` is built fresh per lookup, which is
/// what `abilities_for`'s by-value return already requires.
type AbilitiesFn = Box<dyn Fn() -> Vec<Ability> + Send + Sync>;

/// The tables the installed registry's function pointers read.
///
/// A single process-wide [`OnceLock`] is what makes those pointers possible: a
/// `fn(&CardCode) -> …` cannot capture, so the data it consults has to be
/// reachable from a `static`. That the slot is global costs nothing here —
/// [`card_registry::install`] is already process-global and first-install-wins.
///
/// Populated **only** by an [`install`](MockRegistry::install) that won the
/// registry slot, so the invariant every lookup below relies on holds: if
/// `TABLES` is set, the installed registry is this module's.
static TABLES: OnceLock<MockRegistry> = OnceLock::new();

/// Builder for a test binary's mock [`CardRegistry`]. See the [module
/// docs](self) for the shape and for what it deliberately does not provide.
///
/// [`install`](Self::install) composes the two lookups every game-core mock
/// needs anyway — [`metadata_for_test_inv`](super::metadata_for_test_inv) and
/// [`abilities_for_terminal`](super::abilities_for_terminal) — so a caller that
/// uses [`test_investigator`](super::test_investigator) or a
/// [`terminal_code`](super::terminal_code) act/agenda gets them served without
/// naming them.
#[derive(Default)]
pub struct MockRegistry {
    metadata: Vec<CardMetadata>,
    abilities: Vec<(CardCode, AbilitiesFn)>,
    back_abilities: Vec<(CardCode, AbilitiesFn)>,
    native_effects: Vec<(String, NativeEffectFn)>,
    native_eligibilities: Vec<(String, EligibilityFn)>,
    native_conditions: Vec<(String, NativeConditionFn)>,
}

impl MockRegistry {
    /// An empty builder: every lookup resolves to nothing but the composed
    /// [`install`](Self::install) defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `metadata` under its own [`CardMetadata::code`].
    ///
    /// The builder owns the value and the installed lookup hands out a
    /// `&'static` borrow of it from the module's process-wide table — which is
    /// the whole reason the hand-rolled version needed a
    /// `OnceLock<CardMetadata>` per card.
    #[must_use]
    pub fn with_card(mut self, metadata: CardMetadata) -> Self {
        self.metadata.push(metadata);
        self
    }

    /// Register the front-side abilities served for `code`.
    #[must_use]
    pub fn with_abilities(
        mut self,
        code: impl Into<String>,
        abilities: impl Fn() -> Vec<Ability> + Send + Sync + 'static,
    ) -> Self {
        self.abilities
            .push((CardCode::new(code), Box::new(abilities)));
        self
    }

    /// Register the **reverse-side** abilities served for `code`
    /// ([`CardRegistry::back_abilities_for`]) — in effect exactly while a
    /// location is unrevealed (#774), so a probe that models an unrevealed
    /// location declares its barrier here and not on
    /// [`with_abilities`](Self::with_abilities).
    #[must_use]
    pub fn with_back_abilities(
        mut self,
        code: impl Into<String>,
        abilities: impl Fn() -> Vec<Ability> + Send + Sync + 'static,
    ) -> Self {
        self.back_abilities
            .push((CardCode::new(code), Box::new(abilities)));
        self
    }

    /// Register the card-local Rust effect served for the
    /// [`Effect::Native`](crate::dsl::Effect::Native) `tag`.
    #[must_use]
    pub fn with_native_effect(mut self, tag: impl Into<String>, effect: NativeEffectFn) -> Self {
        self.native_effects.push((tag.into(), effect));
        self
    }

    /// Register the reaction-eligibility predicate served for `tag`
    /// ([`Ability::eligibility`](crate::dsl::Ability::eligibility)).
    #[must_use]
    pub fn with_native_eligibility(
        mut self,
        tag: impl Into<String>,
        eligibility: EligibilityFn,
    ) -> Self {
        self.native_eligibilities.push((tag.into(), eligibility));
        self
    }

    /// Register the condition predicate served for the
    /// [`Condition::Native`](crate::dsl::Condition::Native) `tag`.
    ///
    /// This method exists to carry the binaries that already mock the slot; it
    /// is **not** an invitation to reach for it. `TODO(#609)`:
    /// [`CardRegistry::native_condition_for`] is expected to be deleted along
    /// with `Condition::Native`, and this method goes with it.
    #[must_use]
    pub fn with_native_condition(
        mut self,
        tag: impl Into<String>,
        condition: NativeConditionFn,
    ) -> Self {
        self.native_conditions.push((tag.into(), condition));
        self
    }

    /// Install this registry into the process-global slot, composing
    /// [`metadata_for_test_inv`](super::metadata_for_test_inv) and
    /// [`abilities_for_terminal`](super::abilities_for_terminal) ahead of the
    /// registered lookups.
    ///
    /// **First install wins**, matching
    /// [`card_registry::install`] and
    /// [`install_registry_with_terminal_cards`](super::install_registry_with_terminal_cards):
    /// a later call is a silent no-op rather than a panic, so a `#[ctor]` and a
    /// belt-and-braces call from a test body can coexist. That holds against
    /// *any* earlier claimant, not just another `MockRegistry` — the tables are
    /// stored only when this call won the slot, so a builder that lost leaves
    /// nothing behind for a lookup to serve from a registry that isn't ours.
    ///
    /// The literal below names all six slots rather than spreading
    /// [`CardRegistry::EMPTY`], so a slot added later is a compile error here —
    /// which is what a builder wants, since every slot is one it must decide
    /// whether to expose a `with_*` for. (`EMPTY` is the right base for a
    /// *partial* literal; this one is total.)
    ///
    /// The winning call publishes the registry a step before its tables, so a
    /// lookup racing the install proper would see an empty one. Nothing does:
    /// an install runs from a `#[ctor]` or from the top of a test, both of them
    /// ahead of any card lookup in the binary.
    pub fn install(self) {
        let won = card_registry::install(CardRegistry {
            metadata_for,
            abilities_for,
            back_abilities_for,
            native_effect_for,
            native_eligibility_for,
            native_condition_for,
        })
        .is_ok();
        if won {
            let _ = TABLES.set(self);
        }
    }
}

/// Serve `tag` out of one of the `Copy`-valued native tables. The three native
/// slots differ only in the function type they carry, so they share this scan.
fn find_tag<T: Copy>(table: &[(String, T)], tag: &str) -> Option<T> {
    table
        .iter()
        .find(|(registered, _)| registered == tag)
        .map(|(_, value)| *value)
}

/// Serve `code` out of one of the two abilities tables, building the
/// `Vec<Ability>` fresh as `abilities_for`'s signature requires.
fn find_abilities(
    table: &'static [(CardCode, AbilitiesFn)],
    code: &CardCode,
) -> Option<Vec<Ability>> {
    table
        .iter()
        .find(|(registered, _)| registered == code)
        .map(|(_, produce)| produce())
}

fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    super::metadata_for_test_inv(code).or_else(|| {
        TABLES
            .get()?
            .metadata
            .iter()
            .find(|metadata| metadata.code == code.as_str())
    })
}

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    super::abilities_for_terminal(code).or_else(|| find_abilities(&TABLES.get()?.abilities, code))
}

fn back_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    find_abilities(&TABLES.get()?.back_abilities, code)
}

fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    find_tag(&TABLES.get()?.native_effects, tag)
}

fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    find_tag(&TABLES.get()?.native_eligibilities, tag)
}

fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    find_tag(&TABLES.get()?.native_conditions, tag)
}
