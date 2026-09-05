//! A builder for the mock [`CardRegistry`] a test binary installs.
//!
//! Thirty-two integration binaries across `game-core` and `cards` hand-roll the
//! same thirty lines: a `OnceLock<CardMetadata>` per probe card so the lookup can
//! hand back a `&'static`, an `abilities_for` match over a handful of invented
//! codes, and a constructor-time install of a `CardRegistry` literal spread over
//! [`CardRegistry::EMPTY`]. [`MockRegistry`] collapses that plumbing:
//!
//! ```ignore
//! #[ctor::ctor(unsafe)]
//! fn install() {
//!     MockRegistry::new()
//!         .with_card(asset_metadata(TRINKET, "Mock Trinket", "…"))
//!         .with_abilities(TRINKET, || vec![activated(Cost::DiscardSelf, gain_resources(1))])
//!         .install();
//! }
//! ```
//!
//! **It collapses plumbing only.** Per [ADR 0016] a synthetic fixture models an
//! engine primitive and never impersonates a printed card, and probe cards stay
//! test-local — one per reader, defined in the file that reads them. So this
//! module exposes no named probe cards and no library of them: every code and
//! every `CardMetadata` here comes from the caller. A shared probe library would
//! recreate, in a new location, the shared-fixture problem the migration exists
//! to undo.
//!
//! [ADR 0016]: https://github.com/talelburg/eldritch/blob/main/docs/adr/0016-a-synthetic-fixture-models-a-primitive-never-a-printed-card.md

use std::sync::OnceLock;

use crate::card_data::CardMetadata;
use crate::card_registry::{
    self, CardRegistry, EligibilityFn, NativeConditionFn, NativeEffectFn,
};
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
    /// `&'static` borrow of it from [`TABLES`] — which is the whole reason the
    /// hand-rolled version needed a `OnceLock<CardMetadata>` per card.
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
    /// a second call is a silent no-op rather than a panic, so a `#[ctor]` and a
    /// belt-and-braces call from a test body can coexist.
    pub fn install(self) {
        let _ = TABLES.set(self);
        let _ = card_registry::install(CardRegistry {
            metadata_for,
            abilities_for,
            back_abilities_for,
            native_effect_for,
            native_eligibility_for,
            native_condition_for,
            ..CardRegistry::EMPTY
        });
    }
}

fn tables() -> Option<&'static MockRegistry> {
    TABLES.get()
}

fn lookup(table: &'static [(CardCode, AbilitiesFn)], code: &CardCode) -> Option<Vec<Ability>> {
    table
        .iter()
        .find(|(registered, _)| registered == code)
        .map(|(_, produce)| produce())
}

fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    super::metadata_for_test_inv(code).or_else(|| {
        tables()?
            .metadata
            .iter()
            .find(|metadata| metadata.code == code.as_str())
    })
}

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    super::abilities_for_terminal(code).or_else(|| lookup(&tables()?.abilities, code))
}

fn back_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    lookup(&tables()?.back_abilities, code)
}

fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    let (_, effect) = tables()?.native_effects.iter().find(|(t, _)| t == tag)?;
    Some(*effect)
}

fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    let (_, eligibility) = tables()?
        .native_eligibilities
        .iter()
        .find(|(t, _)| t == tag)?;
    Some(*eligibility)
}

fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    let (_, condition) = tables()?.native_conditions.iter().find(|(t, _)| t == tag)?;
    Some(*condition)
}
