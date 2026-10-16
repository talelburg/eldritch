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
//!   card-local native eligibility condition. The complement of the Parlor's
//!   grant above.
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
use game_core::card_registry::{NativeConditionFn, NativeEffectFn};

pub mod agnes_baker;
pub mod ancient_evils;
pub mod arcane_initiate;
pub mod arcane_studies;
pub mod attic;
pub mod automatic_45;
pub mod backstab;
pub mod barricade;
pub mod baseball_bat;
pub mod beat_cop;
pub mod blinding_light;
pub mod burglary;
pub mod cellar;
pub mod cover_up;
pub mod crypt_chill;
pub mod cunning_distraction;
pub mod daisy_walker;
pub mod daisys_tote_bag;
pub mod dark_memory;
pub mod deduction;
pub mod derringer_41;
pub mod dig_deep;
pub mod dissonant_voices;
pub mod dodge;
pub mod dr_milan_christopher;
pub mod drawn_to_the_flame;
pub mod dynamite_blast;
pub mod elusive;
pub mod emergency_cache;
pub mod evidence;
pub mod fearless;
pub mod first_aid;
pub mod flashlight;
pub mod forbidden_knowledge;
pub mod frozen_in_fear;
pub mod grasping_hands;
pub mod guard_dog;
pub mod guts;
pub mod hard_knocks;
pub mod haunted;
pub mod heirloom_of_hyperborea;
pub mod holy_rosary;
pub mod hospital_debts;
pub mod hyperawareness;
pub mod hypochondria;
pub mod knife;
pub mod leather_coat;
pub mod leo_de_luca;
pub mod lita_chantler;
pub mod look_what_i_found;
pub mod lucky;
pub mod machete;
pub mod magnifying_glass;
pub mod manual_dexterity;
pub mod medical_texts;
pub mod mind_over_matter;
pub mod obscuring_fog;
pub mod old_book_of_lore;
pub mod on_the_lam;
pub mod opportunist;
pub mod overpower;
pub mod parlor;
pub mod perception;
pub mod physical_training;
pub mod pickpocketing;
pub mod psychosis;
pub mod rabbits_foot;
pub mod research_librarian;
pub mod rise_of_the_ghouls;
pub mod roland_38_special;
pub mod roland_banks;
pub mod rotting_remains;
pub mod scavenging;
pub mod scrying;
pub mod shrivelling;
pub mod silver_twilight_acolyte;
pub mod skids_otoole;
pub mod sneak_attack;
pub mod stray_cat;
pub mod survival_instinct;
pub mod switchblade;
pub mod the_barrier;
pub mod the_necronomicon;
pub mod theyre_getting_out;
pub mod trapped;
pub mod unexpected_courage;
pub mod vicious_blow;
pub mod ward_of_protection;
pub mod wendy_adams;
pub mod wendys_amulet;
pub mod what_have_you_done;
pub mod whats_going_on;
pub mod working_a_hunch;

/// One card's whole registration: its code, the abilities on each side, and
/// the native fns its abilities name by tag. Each card module exports one as
/// `CARD`, listed once in [`ALL`]; every lookup below is a search over it.
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
    /// [`Condition::Native`] for its consumers and `TODO(#609)`, the promotion
    /// that retires the compound, target-referencing ones.
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

/// Every implemented card's registration, one entry per card module. A card
/// is playable iff it is listed here.
pub const ALL: &[CardRecord] = &[
    agnes_baker::CARD,
    ancient_evils::CARD,
    arcane_initiate::CARD,
    arcane_studies::CARD,
    attic::CARD,
    automatic_45::CARD,
    backstab::CARD,
    barricade::CARD,
    baseball_bat::CARD,
    beat_cop::CARD,
    blinding_light::CARD,
    burglary::CARD,
    cellar::CARD,
    cover_up::CARD,
    crypt_chill::CARD,
    cunning_distraction::CARD,
    daisy_walker::CARD,
    daisys_tote_bag::CARD,
    dark_memory::CARD,
    deduction::CARD,
    derringer_41::CARD,
    dig_deep::CARD,
    dissonant_voices::CARD,
    dodge::CARD,
    dr_milan_christopher::CARD,
    drawn_to_the_flame::CARD,
    dynamite_blast::CARD,
    elusive::CARD,
    emergency_cache::CARD,
    evidence::CARD,
    fearless::CARD,
    first_aid::CARD,
    flashlight::CARD,
    forbidden_knowledge::CARD,
    frozen_in_fear::CARD,
    grasping_hands::CARD,
    guard_dog::CARD,
    guts::CARD,
    haunted::CARD,
    hard_knocks::CARD,
    heirloom_of_hyperborea::CARD,
    hospital_debts::CARD,
    holy_rosary::CARD,
    hypochondria::CARD,
    hyperawareness::CARD,
    knife::CARD,
    leather_coat::CARD,
    leo_de_luca::CARD,
    lita_chantler::CARD,
    look_what_i_found::CARD,
    lucky::CARD,
    machete::CARD,
    magnifying_glass::CARD,
    manual_dexterity::CARD,
    medical_texts::CARD,
    mind_over_matter::CARD,
    obscuring_fog::CARD,
    old_book_of_lore::CARD,
    on_the_lam::CARD,
    opportunist::CARD,
    overpower::CARD,
    parlor::CARD,
    perception::CARD,
    physical_training::CARD,
    pickpocketing::CARD,
    psychosis::CARD,
    rabbits_foot::CARD,
    research_librarian::CARD,
    rise_of_the_ghouls::CARD,
    roland_38_special::CARD,
    roland_banks::CARD,
    rotting_remains::CARD,
    scavenging::CARD,
    scrying::CARD,
    shrivelling::CARD,
    silver_twilight_acolyte::CARD,
    skids_otoole::CARD,
    sneak_attack::CARD,
    stray_cat::CARD,
    survival_instinct::CARD,
    switchblade::CARD,
    the_barrier::CARD,
    the_necronomicon::CARD,
    theyre_getting_out::CARD,
    trapped::CARD,
    unexpected_courage::CARD,
    vicious_blow::CARD,
    ward_of_protection::CARD,
    wendy_adams::CARD,
    wendys_amulet::CARD,
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

/// Resolve a [`Condition::Native`](card_dsl::dsl::Condition::Native) tag to its
/// card-local predicate; returns `None` for unregistered tags. See
/// [`CardRecord::conditions`] before registering one.
#[must_use]
pub fn native_condition_for(tag: &str) -> Option<NativeConditionFn> {
    registered_native(tag, |record| record.native_conditions)
}
