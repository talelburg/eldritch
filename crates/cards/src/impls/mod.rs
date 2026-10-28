//! Hand-implemented card effects.
//!
//! Each implemented card lives in its own submodule, exposing a
//! [`CODE`](holy_rosary::CODE) constant, an `abilities()` function
//! returning that card's [`Vec<Ability>`](card_dsl::dsl::Ability), and a
//! `CARD` [`CardRecord`] naming everything the card registers.
//!
//! # Adding a card
//!
//! 1. Write `crates/cards/src/impls/<name>.rs` with `CODE`, `abilities()`
//!    and `pub const CARD: CardRecord = CardRecord::new(CODE, abilities)`.
//!    Chain [`back`](CardRecord::back) for a printed reverse, and
//!    [`effects`](CardRecord::effects) / [`conditions`](CardRecord::conditions)
//!    for each native tag an ability names — an eligibility's included. The native fns stay private to the module.
//! 2. Add `<name>` to the `cards!` list below, in alphabetical order. The
//!    list declares the module and adds its `CARD` to [`ALL`].
//!
//! Spell a native tag `<code>:<kebab-case>`, prefixed with the card's own
//! code. The crate's corpus tests then check the registration: every tag an
//! ability names resolves in its own namespace, every registered native is
//! referenced, no code or tag is listed twice, and the `cards!` list names
//! exactly the module files here, alphabetically.
//! [`is_playable`](super::is_playable) is membership in `ALL`, so
//! playability and registration cannot disagree.
//!
//! # Module-naming convention
//!
//! Filenames are the card's lowercase snake-case name. When two
//! printings share a name (revised core / Chapter 2 reprints), the
//! later printings get a set suffix: `holy_rosary` for the original
//! core, `holy_rosary_rcore` if a revised-core variant lands. Codes
//! are the disambiguator at the registry level; filenames just stay
//! greppable.
//!
//! # Trigger-shape examples
//!
//! The authoritative list of implemented cards is [`ALL`] — this list is NOT
//! exhaustive; it survives as a worked example per trigger shape:
//!
//! - Holy Rosary (01059) — `Trigger::Constant` + unqualified
//!   `WhileInPlay`.
//! - Working a Hunch (01037) — `Trigger::OnPlay` + `DiscoverClue`.
//! - Magnifying Glass (01030) — `Trigger::Constant` +
//!   `WhileInPlayDuring(SkillTestKind::Investigate)`.
//! - Hyperawareness (01034) — two undesignated `Trigger::Activated
//!   { action_cost: 0 }` abilities with `Cost::Resources(1)` and
//!   `ThisSkillTest`-scoped `Modify`.
//! - .45 Automatic (01016) — `Trigger::Activated { action_cost: 1 }` with the
//!   **Fight** action designator carrying the modification (flat +1 combat,
//!   +1 damage) and `Cost::SpendUses(Ammo)`.
//! - Physical Training (01017) — two undesignated `Trigger::Activated
//!   { action_cost: 0 }` abilities (`Cost::Resources(1)`, `ThisSkillTest`
//!   `Modify`), willpower / combat (the Hyperawareness shape).
//! - Machete (01020) — bare `Trigger::Activated { action_cost: 1 }` with the
//!   **Fight** action designator carrying the modification (+1 combat;
//!   conditional +1 damage via `IntExpr::cond` over a `Condition::Native` predicate that
//!   reads the *attacked* enemy, since the fight scope is every co-located
//!   enemy).
//! - Attic (01113) — `Trigger::OnEvent` (`EnteredLocation`, `After`) +
//!   `deal_horror(You, 1)`.
//! - Cellar (01114) — `Trigger::OnEvent` (`EnteredLocation`, `After`) +
//!   `deal_damage(You, 1)`.
//! - Parlor (01115) — `Trigger::Activated { action_cost: 1 }` with the
//!   nullary **Resign** action designator, which performs the elimination;
//!   and the corpus's only **back-side** abilities (its record's [`back`](CardRecord::back)),
//!   a `Trigger::Constant` `Restrict` that blocks investigator movement while
//!   the location is unrevealed.
//! - Lita Chantler (01117) — a `Trigger::Constant` `Effect::Grant` to
//!   **herself**, gated on `ControlStatus::ByAPlayer`, holding the two
//!   abilities she gains: a location-scoped `Modify` (the corpus's first
//!   *granted* modifier) and an `OnEvent` reaction on another investigator's
//!   test (`SkillTestResolved { by_controller: false }`) carrying a
//!   card-local native eligibility condition. The complement of the Parlor's
//!   grant above.
//! - Deduction (01039) — `Trigger::OnCommit` +
//!   `If(SkillTestKind(Investigate), DiscoverAdditionalClues(1))`.
//! - Roland Banks (01001) — investigator. `Trigger::OnEvent`
//!   reaction (`EnemyDefeated { by_controller: true }`, `After`) +
//!   `UsageLimit { count: 1, period: Round }` for "Limit once per
//!   round." Elder-sign half: `Trigger::ElderSign` over
//!   `Count(CluesAtControllerLocation)`.
//! - Trapped (01108) — Act 1; `Trigger::OnEvent` (`ActAdvanced`, `After`) on-advance board build.
//! - The Barrier (01109) — Act 2; `Trigger::OnEvent` (`ActAdvanced`, `After`) on-advance reverse: reveal the Parlor + spawn the set-aside Ghoul Priest.
//! - What Have You Done? (01110) — Act 3; `Trigger::OnEvent` (`EnemyDefeated` 01116, `At`) -> `AdvanceCurrentAct`.
//! - What's Going On?! (01105) — Agenda 1; `Trigger::OnEvent` (`AgendaAdvanced`, `After`) reverse: the lead investigator's interactive `ChooseOne` (each investigator discards 1 random card, or the lead takes 2 horror).
//! - Rise of the Ghouls (01106) — Agenda 2; `Trigger::OnEvent` (`AgendaAdvanced`, `After`) reverse: dig the encounter deck until a Ghoul, which the lead draws.

use card_dsl::dsl::Ability;
use game_core::card_registry::{NativeConditionFn, NativeEffectFn};

/// Declares every card module, and builds [`ALL`] and `MODULES` from the same
/// list, so a module name is written once.
macro_rules! cards {
    ($($name:ident),* $(,)?) => {
        $(pub mod $name;)*

        /// Every implemented card's registration, one entry per card module.
        /// A card is playable iff it is listed here.
        pub const ALL: &[CardRecord] = &[$($name::CARD),*];

        /// Every card module's name, in list order, for the corpus test that
        /// checks the list against the module files.
        #[cfg(test)]
        pub(crate) const MODULES: &[&str] = &[$(stringify!($name)),*];
    };
}

cards! {
    abandoned_and_alone,
    agnes_baker,
    amnesia,
    ancient_evils,
    arcane_initiate,
    arcane_studies,
    attic,
    automatic_45,
    backstab,
    barricade,
    baseball_bat,
    beat_cop,
    blinding_light,
    burglary,
    cellar,
    cover_up,
    crypt_chill,
    cunning_distraction,
    daisy_walker,
    daisys_tote_bag,
    dark_memory,
    deduction,
    derringer_41,
    dig_deep,
    dissonant_voices,
    dodge,
    dr_milan_christopher,
    drawn_to_the_flame,
    dynamite_blast,
    elusive,
    emergency_cache,
    evidence,
    fearless,
    first_aid,
    flashlight,
    forbidden_knowledge,
    frozen_in_fear,
    grasping_hands,
    guard_dog,
    guts,
    hard_knocks,
    haunted,
    heirloom_of_hyperborea,
    holy_rosary,
    hospital_debts,
    hyperawareness,
    hypochondria,
    knife,
    leather_coat,
    leo_de_luca,
    lita_chantler,
    look_what_i_found,
    lucky,
    machete,
    magnifying_glass,
    manual_dexterity,
    medical_texts,
    mind_over_matter,
    mob_enforcer,
    obscuring_fog,
    old_book_of_lore,
    on_the_lam,
    opportunist,
    overpower,
    paranoia,
    parlor,
    perception,
    physical_training,
    pickpocketing,
    psychosis,
    rabbits_foot,
    research_librarian,
    rise_of_the_ghouls,
    roland_38_special,
    roland_banks,
    rotting_remains,
    scavenging,
    scrying,
    shrivelling,
    silver_twilight_acolyte,
    skids_otoole,
    sneak_attack,
    stray_cat,
    stubborn_detective,
    survival_instinct,
    switchblade,
    the_barrier,
    the_necronomicon,
    theyre_getting_out,
    trapped,
    unexpected_courage,
    vicious_blow,
    ward_of_protection,
    wendy_adams,
    wendys_amulet,
    what_have_you_done,
    whats_going_on,
    working_a_hunch,
}

/// One card's whole registration: its code, the abilities on each side, and the
/// native fns its abilities name by tag.
///
/// Each card module exports one as `CARD`, listed once in [`ALL`]; every lookup
/// below is a search over it.
///
/// The two native namespaces stay separate because the engine resolves them
/// through separate [`CardRegistry`](game_core::card_registry::CardRegistry)
/// slots — an effect mutates, a condition only reads — so a tag listed in the
/// wrong slice is never found. Build one with
/// [`CardRecord::new`] and chain only the setters the card needs.
#[derive(Debug, Clone, Copy)]
pub struct CardRecord {
    /// The card's `ArkhamDB` code.
    pub code: &'static str,
    /// The abilities printed on the card's front.
    pub abilities: fn() -> Vec<Ability>,
    /// The abilities printed on its reverse, for a card that has any.
    pub back_abilities: Option<fn() -> Vec<Ability>>,
    /// [`Effect::Native`](card_dsl::dsl::Effect::Native) tags and their fns.
    pub native_effects: &'static [(&'static str, NativeEffectFn)],
    /// [`Condition::Native`](card_dsl::dsl::Condition::Native) tags and their
    /// predicates.
    pub native_conditions: &'static [(&'static str, NativeConditionFn)],
}

impl CardRecord {
    /// A card with front abilities and nothing else registered.
    #[must_use]
    pub const fn new(code: &'static str, abilities: fn() -> Vec<Ability>) -> Self {
        Self {
            code,
            abilities,
            back_abilities: None,
            native_effects: &[],
            native_conditions: &[],
        }
    }

    /// Register the abilities printed on the card's reverse.
    #[must_use]
    pub const fn back(mut self, back_abilities: fn() -> Vec<Ability>) -> Self {
        self.back_abilities = Some(back_abilities);
        self
    }

    /// Register the card's native effects.
    #[must_use]
    pub const fn effects(mut self, effects: &'static [(&'static str, NativeEffectFn)]) -> Self {
        self.native_effects = effects;
        self
    }

    /// Register the card's native conditions: every read-only predicate the
    /// card names by tag, whether a [`Condition::Native`] inside an effect or
    /// an ability's [`eligibility`](Ability::eligibility).
    ///
    /// Before registering one, check the DSL can't say it — see
    /// [`Condition::Native`] for its consumers and the promotion that retires
    /// the compound, target-referencing ones (TODO(#609): promote them to
    /// declarative vocab).
    ///
    /// [`Condition::Native`]: card_dsl::dsl::Condition::Native
    #[must_use]
    pub const fn conditions(
        mut self,
        conditions: &'static [(&'static str, NativeConditionFn)],
    ) -> Self {
        self.native_conditions = conditions;
        self
    }
}

/// The record registered for `code`.
pub(crate) fn record(code: &str) -> Option<&'static CardRecord> {
    ALL.iter().find(|record| record.code == code)
}

/// Look up a card's hand-implemented abilities by code. Returns `None` for
/// unimplemented cards.
#[must_use]
pub fn abilities_for(code: &str) -> Option<Vec<Ability>> {
    record(code).map(|record| (record.abilities)())
}

/// Look up the abilities printed on a card's **reverse side** by code. Returns
/// `None` for a card with no implemented back-side abilities.
///
/// A separate lookup from [`abilities_for`], not a second arm of it: which
/// side is in effect is the engine's question (for a location, its `revealed`
/// flag — `game_core::engine::abilities_in_effect`), and a card declares only
/// what each side says. The Parlor 01115 is the sole card with a back today;
/// its back carries the barrier that blocks investigators until act 01109b
/// reveals the location.
#[must_use]
pub fn back_abilities_for(code: &str) -> Option<Vec<Ability>> {
    record(code)?.back_abilities.map(|back| back())
}

/// The fn registered under `tag` in the namespace `namespace` picks out of
/// each record.
fn registered_native<F: Copy>(
    tag: &str,
    namespace: fn(&CardRecord) -> &'static [(&'static str, F)],
) -> Option<F> {
    ALL.iter()
        .flat_map(namespace)
        .find(|(registered, _)| *registered == tag)
        .map(|(_, f)| *f)
}

/// Resolve an [`Effect::Native`](card_dsl::dsl::Effect::Native) tag to the
/// card-local Rust fn that implements it; returns `None` for unregistered tags.
#[must_use]
pub fn native_effect_for(tag: &str) -> Option<NativeEffectFn> {
    registered_native(tag, |record| record.native_effects)
}

/// Resolve a [`Condition::Native`](card_dsl::dsl::Condition::Native) tag to its
/// card-local predicate; returns `None` for unregistered tags. See
/// [`CardRecord::conditions`] before registering one.
#[must_use]
pub fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    registered_native(tag, |record| record.native_conditions)
}
