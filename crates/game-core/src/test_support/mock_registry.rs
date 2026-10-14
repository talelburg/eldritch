//! The mock card registry a test binary builds; see [`MockRegistry`].

use std::sync::OnceLock;

use card_dsl::card_data::CardMetadata;
use card_dsl::dsl::Ability;

use crate::card_registry::{CardRegistry, NativeConditionFn, NativeEffectFn};
use crate::state::CardCode;
use crate::test_support;

/// A per-code abilities producer. Boxed rather than a `fn` pointer so a caller
/// can close over locals — a `Vec<Ability>` is built fresh per lookup, which is
/// what `abilities_for`'s by-value return already requires.
type AbilitiesFn = Box<dyn Fn() -> Vec<Ability> + Send + Sync>;

/// The tables the built registry's function pointers read.
///
/// A single process-wide [`OnceLock`] is what makes those pointers possible: a
/// `fn(&CardCode) -> …` cannot capture, so the data it consults has to be
/// reachable from a `static`. The cost is one mock per process, which
/// [`build`](MockRegistry::build) enforces.
static TABLES: OnceLock<MockRegistry> = OnceLock::new();

/// A builder for the mock [`CardRegistry`] a test binary names.
///
/// Integration binaries across `game-core` and `cards` hand-roll the same
/// plumbing: a `OnceLock<CardMetadata>` per probe card so the lookup can hand
/// back a `&'static`, an `abilities_for` match over a handful of invented codes,
/// and a `CardRegistry` literal spread over
/// [`CardRegistry::EMPTY`]. [`MockRegistry`] collapses that plumbing.
///
/// ```
/// use card_dsl::dsl::{self, ModifierScope, Stat};
/// use game_core::test_support::MockRegistry;
///
/// let cards = MockRegistry::new()
///     .with_abilities("_doc_probe", || {
///         vec![dsl::constant(dsl::modify(
///             Stat::Willpower,
///             1,
///             ModifierScope::WhileInPlay,
///         ))]
///     })
///     .build();
/// # assert!((cards.abilities_for)(&"_doc_probe".into()).is_some());
/// ```
///
/// A card's metadata rides [`with_card`](MockRegistry::with_card), which takes a
/// `CardMetadata` the caller builds — there is no shared constructor for one,
/// deliberately.
///
/// **It collapses plumbing only.** Per [ADR 0016] a synthetic fixture models an
/// engine primitive and never impersonates a printed card, and probe cards stay
/// test-local — one per reader, defined in the file that reads them. So this
/// module exposes no named probe cards and no library of them: every code and
/// every `CardMetadata` here comes from the caller. A shared probe library would
/// recreate, in a new location, the shared-fixture problem that the test-substrate
/// migration ([#864]) exists to undo.
///
/// [ADR 0016]: https://github.com/talelburg/eldritch/blob/main/docs/adr/0016-a-synthetic-fixture-models-a-primitive-never-a-printed-card.md
/// [#864]: https://github.com/talelburg/eldritch/issues/864
///
/// [`build`](Self::build) composes the two lookups every game-core mock
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
    native_conditions: Vec<(String, NativeConditionFn)>,
}

impl MockRegistry {
    /// An empty builder: every lookup resolves to nothing but the composed
    /// [`build`](Self::build) defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `metadata` under its own [`CardMetadata::code`].
    ///
    /// The builder owns the value and the built lookup hands out a
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
    /// [`Effect::Native`](card_dsl::dsl::Effect::Native) `tag`.
    #[must_use]
    pub fn with_native_effect(mut self, tag: impl Into<String>, effect: NativeEffectFn) -> Self {
        self.native_effects.push((tag.into(), effect));
        self
    }

    /// Register the condition predicate served for the
    /// [`Condition::Native`](card_dsl::dsl::Condition::Native) `tag` — whether
    /// it guards an effect or an ability's
    /// [`eligibility`](card_dsl::dsl::Ability::eligibility), since both resolve
    /// in [`CardRegistry::native_condition_for`].
    #[must_use]
    pub fn with_native_condition(
        mut self,
        tag: impl Into<String>,
        condition: NativeConditionFn,
    ) -> Self {
        self.native_conditions.push((tag.into(), condition));
        self
    }

    /// This registry as a value, for a test to name — on the board
    /// ([`with_registries`](crate::state::GameStateBuilder::with_registries))
    /// or the session. It composes
    /// [`metadata_for_test_inv`](super::metadata_for_test_inv) and
    /// [`abilities_for_terminal`](super::abilities_for_terminal) ahead of the
    /// registered lookups.
    ///
    /// The lookups still read one process-wide table (a `fn` pointer cannot
    /// capture), so a process holds **one** mock. Build it once per test
    /// binary and cache it:
    ///
    /// ```
    /// use std::sync::LazyLock;
    /// use game_core::card_registry::CardRegistry;
    /// use game_core::test_support::MockRegistry;
    ///
    /// fn cards() -> &'static CardRegistry {
    ///     static CARDS: LazyLock<&'static CardRegistry> =
    ///         LazyLock::new(|| MockRegistry::new().build());
    ///     *CARDS
    /// }
    /// # let _ = cards();
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if a mock was already built in this process: a
    /// second table would otherwise be served silently from the first.
    #[must_use]
    pub fn build(self) -> &'static CardRegistry {
        assert!(
            TABLES.set(self).is_ok(),
            "MockRegistry::build: a mock registry was already built in this process; \
             build one per test binary and cache it",
        );
        &MOCK
    }
}

/// The registry every mock serves through: all five slots read [`TABLES`].
///
/// The literal names all five slots rather than spreading
/// [`CardRegistry::EMPTY`], so a slot added later is a compile error here —
/// which is what a builder wants, since every slot is one it must decide
/// whether to expose a `with_*` for. (`EMPTY` is the right base for a
/// *partial* literal; this one is total.)
static MOCK: CardRegistry = CardRegistry {
    metadata_for,
    abilities_for,
    back_abilities_for,
    native_effect_for,
    native_condition_for,
};

/// Serve `tag` out of one of the `Copy`-valued native tables. The two native
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
    test_support::metadata_for_test_inv(code).or_else(|| {
        TABLES
            .get()?
            .metadata
            .iter()
            .find(|metadata| metadata.code == code.as_str())
    })
}

fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    test_support::abilities_for_terminal(code)
        .or_else(|| find_abilities(&TABLES.get()?.abilities, code))
}

fn back_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    find_abilities(&TABLES.get()?.back_abilities, code)
}

fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    find_tag(&TABLES.get()?.native_effects, tag)
}

fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    find_tag(&TABLES.get()?.native_conditions, tag)
}
