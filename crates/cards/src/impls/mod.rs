//! Hand-implemented card effects.
//!
//! Each implemented card lives in its own submodule, exposing a
//! [`CODE`](holy_rosary::CODE) constant and an `abilities()` function
//! returning that card's [`Vec<Ability>`](card_dsl::dsl::Ability).
//!
//! The registry is the [`abilities_for`] dispatch — adding a card
//! means: drop a `crates/cards/src/impls/<name>.rs` file, declare the
//! `pub mod <name>;` here, and add a match arm in [`abilities_for`].
//! The crate's [`is_playable`](super::is_playable) check derives from
//! `abilities_for(code).is_some()`, so the two queries can never go
//! out of sync.
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
//! # Implemented so far
//!
//! - Holy Rosary (01059) — `Trigger::Constant` + unqualified
//!   `WhileInPlay`.
//! - Working a Hunch (01037) — `Trigger::OnPlay` + `DiscoverClue`.
//! - Magnifying Glass (01030) — `Trigger::Constant` +
//!   `WhileInPlayDuring(SkillTestKind::Investigate)`.
//! - Hyperawareness (01034) — two `Trigger::Activated { action_cost: 0 }`
//!   abilities with `Cost::Resources(1)` and `ThisSkillTest`-scoped
//!   `Modify`.
//! - .45 Automatic (01016) — `Trigger::Activated { action_cost: 1 }`,
//!   `Cost::SpendUses(Ammo)` + `Effect::Fight` (flat +1 combat, +1 damage).
//! - Physical Training (01017) — two `Trigger::Activated { action_cost: 0 }`
//!   abilities (`Cost::Resources(1)`, `ThisSkillTest` `Modify`), willpower
//!   / combat (the Hyperawareness shape).
//! - Machete (01020) — bare `Trigger::Activated { action_cost: 1 }` +
//!   `Effect::Fight` (flat +1 combat, +1 damage; conditional-damage caveat
//!   in the impl, TODO(#300)).
//! - Attic (01113) — `Trigger::OnEvent` (`EnteredLocation`, `After`) +
//!   `deal_horror(You, 1)`.
//! - Cellar (01114) — `Trigger::OnEvent` (`EnteredLocation`, `After`) +
//!   `deal_damage(You, 1)`.
//! - Deduction (01039) — `Trigger::OnSkillTestResolution` (Success-
//!   gated) + `If(SkillTestKind(Investigate), DiscoverClue@TestedLocation)`.
//! - Roland Banks (01001) — investigator. `Trigger::OnEvent`
//!   reaction (`EnemyDefeated { by_controller: true }`, `After`) +
//!   `UsageLimit { count: 1, period: Round }` for "Limit once per
//!   round." Elder-sign half stubbed pending #118.
//! - Trapped (01108) — Act 1; `Trigger::OnEvent` (`ActAdvanced`, `After`) on-advance board build.
//! - The Barrier (01109) — Act 2; `Trigger::OnEvent` (`ActAdvanced`, `After`) on-advance reverse: reveal the Parlor + spawn the set-aside Ghoul Priest.
//! - What Have You Done? (01110) — Act 3; `Trigger::OnEvent` (`EnemyDefeated` 01116, `After`) -> `AdvanceCurrentAct`.
//! - What's Going On?! (01105) — Agenda 1; `Trigger::OnEvent` (`AgendaAdvanced`, `After`) reverse: lead's interactive `ChooseOne` (each investigator discards 1 random card, or lead takes 2 horror) — Axis A #334.
//! - Rise of the Ghouls (01106) — Agenda 2; `Trigger::OnEvent` (`AgendaAdvanced`, `After`) reverse: dig the encounter deck until a Ghoul, lead draws it.
//!
//! The remaining Phase-3 card (Study #56) blocks on the
//! location-state shape.

use card_dsl::dsl::Ability;

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
pub mod machete;
pub mod magnifying_glass;
pub mod manual_dexterity;
pub mod medical_texts;
pub mod mind_over_matter;
pub mod obscuring_fog;
pub mod old_book_of_lore;
pub mod overpower;
pub mod perception;
pub mod physical_training;
pub mod research_librarian;
pub mod rise_of_the_ghouls;
pub mod roland_38_special;
pub mod roland_banks;
pub mod rotting_remains;
pub mod the_barrier;
pub mod theyre_getting_out;
pub mod trapped;
pub mod unexpected_courage;
pub mod vicious_blow;
pub mod what_have_you_done;
pub mod whats_going_on;
pub mod working_a_hunch;

/// Look up a card's hand-implemented abilities by code. Returns
/// `None` for unimplemented cards.
#[must_use]
pub fn abilities_for(code: &str) -> Option<Vec<Ability>> {
    match code {
        ancient_evils::CODE => Some(ancient_evils::abilities()),
        attic::CODE => Some(attic::abilities()),
        automatic_45::CODE => Some(automatic_45::abilities()),
        barricade::CODE => Some(barricade::abilities()),
        beat_cop::CODE => Some(beat_cop::abilities()),
        cellar::CODE => Some(cellar::abilities()),
        cover_up::CODE => Some(cover_up::abilities()),
        crypt_chill::CODE => Some(crypt_chill::abilities()),
        deduction::CODE => Some(deduction::abilities()),
        dissonant_voices::CODE => Some(dissonant_voices::abilities()),
        dodge::CODE => Some(dodge::abilities()),
        dr_milan_christopher::CODE => Some(dr_milan_christopher::abilities()),
        dynamite_blast::CODE => Some(dynamite_blast::abilities()),
        emergency_cache::CODE => Some(emergency_cache::abilities()),
        evidence::CODE => Some(evidence::abilities()),
        first_aid::CODE => Some(first_aid::abilities()),
        flashlight::CODE => Some(flashlight::abilities()),
        frozen_in_fear::CODE => Some(frozen_in_fear::abilities()),
        grasping_hands::CODE => Some(grasping_hands::abilities()),
        guard_dog::CODE => Some(guard_dog::abilities()),
        guts::CODE => Some(guts::abilities()),
        holy_rosary::CODE => Some(holy_rosary::abilities()),
        hyperawareness::CODE => Some(hyperawareness::abilities()),
        knife::CODE => Some(knife::abilities()),
        machete::CODE => Some(machete::abilities()),
        magnifying_glass::CODE => Some(magnifying_glass::abilities()),
        manual_dexterity::CODE => Some(manual_dexterity::abilities()),
        medical_texts::CODE => Some(medical_texts::abilities()),
        mind_over_matter::CODE => Some(mind_over_matter::abilities()),
        obscuring_fog::CODE => Some(obscuring_fog::abilities()),
        old_book_of_lore::CODE => Some(old_book_of_lore::abilities()),
        overpower::CODE => Some(overpower::abilities()),
        perception::CODE => Some(perception::abilities()),
        physical_training::CODE => Some(physical_training::abilities()),
        research_librarian::CODE => Some(research_librarian::abilities()),
        rise_of_the_ghouls::CODE => Some(rise_of_the_ghouls::abilities()),
        roland_38_special::CODE => Some(roland_38_special::abilities()),
        roland_banks::CODE => Some(roland_banks::abilities()),
        rotting_remains::CODE => Some(rotting_remains::abilities()),
        the_barrier::CODE => Some(the_barrier::abilities()),
        theyre_getting_out::CODE => Some(theyre_getting_out::abilities()),
        trapped::CODE => Some(trapped::abilities()),
        unexpected_courage::CODE => Some(unexpected_courage::abilities()),
        vicious_blow::CODE => Some(vicious_blow::abilities()),
        what_have_you_done::CODE => Some(what_have_you_done::abilities()),
        whats_going_on::CODE => Some(whats_going_on::abilities()),
        working_a_hunch::CODE => Some(working_a_hunch::abilities()),
        _ => None,
    }
}

/// Resolve an [`Effect::Native`](card_dsl::dsl::Effect::Native) tag to the
/// card-local Rust fn that implements it. Mirrors [`abilities_for`]'s
/// per-card delegation; returns `None` for unregistered tags.
#[must_use]
pub fn native_effect_for(tag: &str) -> Option<game_core::card_registry::NativeEffectFn> {
    trapped::native_effect_for(tag)
        .or_else(|| the_barrier::native_effect_for(tag))
        .or_else(|| whats_going_on::native_effect_for(tag))
        .or_else(|| rise_of_the_ghouls::native_effect_for(tag))
        .or_else(|| theyre_getting_out::native_effect_for(tag))
        .or_else(|| dynamite_blast::native_effect_for(tag))
        .or_else(|| guard_dog::native_effect_for(tag))
        .or_else(|| mind_over_matter::native_effect_for(tag))
        .or_else(|| cover_up::native_effect_for(tag))
        .or_else(|| ancient_evils::native_effect_for(tag))
        .or_else(|| crypt_chill::native_effect_for(tag))
        .or_else(|| obscuring_fog::native_effect_for(tag))
}

/// Dispatch a native eligibility-predicate tag to its card-local handler;
/// returns `None` for unregistered tags.
#[must_use]
pub fn native_eligibility_for(tag: &str) -> Option<game_core::card_registry::EligibilityFn> {
    cover_up::native_eligibility_for(tag).or_else(|| the_barrier::native_eligibility_for(tag))
}
