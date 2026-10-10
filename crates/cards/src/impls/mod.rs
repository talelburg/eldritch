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
//!    [`effects`](CardRecord::effects) / [`eligibility`](CardRecord::eligibility)
//!    / [`conditions`](CardRecord::conditions) for each native tag an ability
//!    names. The native fns stay private to the module.
//! 2. Declare `pub mod <name>;` below.
//! 3. Add `<name>::CARD` to [`ALL`].
//!
//! Spell a native tag `<code>:<kebab-case>`, prefixed with the card's own
//! code. The crate's corpus tests then check the registration: every tag an
//! ability names resolves in its own namespace, every registered native is
//! referenced, no code or tag is listed twice, and every module file here has
//! an `ALL` entry. [`is_playable`](super::is_playable) is membership in
//! `ALL`, so playability and registration cannot disagree.
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
//!   enemy — #451/#592).
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
//!   card-local eligibility tag. The complement of the Parlor's grant above.
//! - Deduction (01039) — `Trigger::OnSkillTestResolution` (Success-
//!   gated) + `If(SkillTestKind(Investigate), DiscoverClue@TestedLocation)`.
//! - Roland Banks (01001) — investigator. `Trigger::OnEvent`
//!   reaction (`EnemyDefeated { by_controller: true }`, `After`) +
//!   `UsageLimit { count: 1, period: Round }` for "Limit once per
//!   round." Elder-sign half stubbed pending #118.
//! - Trapped (01108) — Act 1; `Trigger::OnEvent` (`ActAdvanced`, `After`) on-advance board build.
//! - The Barrier (01109) — Act 2; `Trigger::OnEvent` (`ActAdvanced`, `After`) on-advance reverse: reveal the Parlor + spawn the set-aside Ghoul Priest.
//! - What Have You Done? (01110) — Act 3; `Trigger::OnEvent` (`EnemyDefeated` 01116, `At`) -> `AdvanceCurrentAct`.
//! - What's Going On?! (01105) — Agenda 1; `Trigger::OnEvent` (`AgendaAdvanced`, `After`) reverse: lead's interactive `ChooseOne` (each investigator discards 1 random card, or lead takes 2 horror) — Axis A #334.
//! - Rise of the Ghouls (01106) — Agenda 2; `Trigger::OnEvent` (`AgendaAdvanced`, `After`) reverse: dig the encounter deck until a Ghoul, lead draws it.

use card_dsl::dsl::Ability;
use game_core::card_registry::{EligibilityFn, NativeConditionFn, NativeEffectFn};

pub mod ancient_evils;
pub mod attic;
pub mod automatic_45;
pub mod barricade;
pub mod beat_cop;
pub mod cellar;
pub mod cover_up;
pub mod crypt_chill;
pub mod deduction;
pub mod dissonant_voices;
pub mod dodge;
pub mod dr_milan_christopher;
pub mod dynamite_blast;
pub mod emergency_cache;
pub mod evidence;
pub mod first_aid;
pub mod flashlight;
pub mod frozen_in_fear;
pub mod grasping_hands;
pub mod guard_dog;
pub mod guts;
pub mod holy_rosary;
pub mod hyperawareness;
pub mod knife;
pub mod lita_chantler;
pub mod machete;
pub mod magnifying_glass;
pub mod manual_dexterity;
pub mod medical_texts;
pub mod mind_over_matter;
pub mod obscuring_fog;
pub mod old_book_of_lore;
pub mod overpower;
pub mod parlor;
pub mod perception;
pub mod physical_training;
pub mod research_librarian;
pub mod rise_of_the_ghouls;
pub mod roland_38_special;
pub mod roland_banks;
pub mod rotting_remains;
pub mod silver_twilight_acolyte;
pub mod the_barrier;
pub mod theyre_getting_out;
pub mod trapped;
pub mod unexpected_courage;
pub mod vicious_blow;
pub mod what_have_you_done;
pub mod whats_going_on;
pub mod working_a_hunch;

/// One card's whole registration: its code, the abilities on each side, and
/// the native fns its abilities name by tag. Each card module exports one as
/// `CARD`, listed once in [`ALL`]; every lookup below is a search over it.
///
/// The three native namespaces stay separate because the engine resolves them
/// through separate [`CardRegistry`](game_core::card_registry::CardRegistry)
/// slots: an eligibility predicate and a condition share a signature but not a
/// role, so a tag listed in the wrong slice is never found. Build one with
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
    /// [`Ability::eligibility`] tags and their predicates.
    pub native_eligibility: &'static [(&'static str, EligibilityFn)],
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
            native_eligibility: &[],
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

    /// Register the card's native eligibility predicates.
    #[must_use]
    pub const fn eligibility(
        mut self,
        eligibility: &'static [(&'static str, EligibilityFn)],
    ) -> Self {
        self.native_eligibility = eligibility;
        self
    }

    /// Register the card's native conditions.
    ///
    /// `TODO(#609)`: two cards use this namespace, Machete 01020 and 01107, and
    /// a third should not. Promotion to declarative DSL vocab is triggered by
    /// the next card wanting a **compound or target-referencing** condition,
    /// which is what Machete has and the DSL cannot express. 01107's act-deck
    /// branch is neither: it is a plain scenario-state read (`act_index == 2`),
    /// so it lands here without firing that trigger.
    #[must_use]
    pub const fn conditions(
        mut self,
        conditions: &'static [(&'static str, NativeConditionFn)],
    ) -> Self {
        self.native_conditions = conditions;
        self
    }
}

/// Every implemented card's registration, one entry per card module. A card
/// is playable iff it is listed here.
pub const ALL: &[CardRecord] = &[
    ancient_evils::CARD,
    attic::CARD,
    automatic_45::CARD,
    barricade::CARD,
    beat_cop::CARD,
    cellar::CARD,
    cover_up::CARD,
    crypt_chill::CARD,
    deduction::CARD,
    dissonant_voices::CARD,
    dodge::CARD,
    dr_milan_christopher::CARD,
    dynamite_blast::CARD,
    emergency_cache::CARD,
    evidence::CARD,
    first_aid::CARD,
    flashlight::CARD,
    frozen_in_fear::CARD,
    grasping_hands::CARD,
    guard_dog::CARD,
    guts::CARD,
    holy_rosary::CARD,
    hyperawareness::CARD,
    knife::CARD,
    lita_chantler::CARD,
    machete::CARD,
    magnifying_glass::CARD,
    manual_dexterity::CARD,
    medical_texts::CARD,
    mind_over_matter::CARD,
    obscuring_fog::CARD,
    old_book_of_lore::CARD,
    overpower::CARD,
    parlor::CARD,
    perception::CARD,
    physical_training::CARD,
    research_librarian::CARD,
    rise_of_the_ghouls::CARD,
    roland_38_special::CARD,
    roland_banks::CARD,
    rotting_remains::CARD,
    silver_twilight_acolyte::CARD,
    the_barrier::CARD,
    theyre_getting_out::CARD,
    trapped::CARD,
    unexpected_courage::CARD,
    vicious_blow::CARD,
    what_have_you_done::CARD,
    whats_going_on::CARD,
    working_a_hunch::CARD,
];

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

/// Resolve a native eligibility-predicate tag to its card-local predicate;
/// returns `None` for unregistered tags.
#[must_use]
pub fn native_eligibility_for(tag: &str) -> Option<EligibilityFn> {
    registered_native(tag, |record| record.native_eligibility)
}

/// Resolve a [`Condition::Native`](card_dsl::dsl::Condition::Native) tag to its
/// card-local predicate; returns `None` for unregistered tags. See
/// [`CardRecord::conditions`] before registering one.
#[must_use]
pub fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    registered_native(tag, |record| record.native_conditions)
}
