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
//!   `DealHorror(You, 1)`.
//! - Cellar (01114) — `Trigger::OnEvent` (`EnteredLocation`, `After`) +
//!   `DealDamage(You, 1)`.
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

pub mod act_01108;
pub mod act_01109;
pub mod act_01110;
pub mod agenda_01105;
pub mod agenda_01106;
pub mod agenda_01107;
pub mod attic;
pub mod automatic_45;
pub mod cellar;
pub mod deduction;
pub mod dr_milan_christopher;
pub mod emergency_cache;
pub mod guard_dog;
pub mod guts;
pub mod holy_rosary;
pub mod hyperawareness;
pub mod machete;
pub mod magnifying_glass;
pub mod manual_dexterity;
pub mod overpower;
pub mod perception;
pub mod physical_training;
pub mod roland_38_special;
pub mod roland_banks;
pub mod treachery_01007;
pub mod treachery_01162;
pub mod treachery_01163;
pub mod treachery_01164;
pub mod treachery_01165;
pub mod treachery_01166;
pub mod treachery_01167;
pub mod treachery_01168;
pub mod unexpected_courage;
pub mod vicious_blow;
pub mod working_a_hunch;

/// Look up a card's hand-implemented abilities by code. Returns
/// `None` for unimplemented cards.
#[must_use]
pub fn abilities_for(code: &str) -> Option<Vec<Ability>> {
    match code {
        act_01108::CODE => Some(act_01108::abilities()),
        act_01109::CODE => Some(act_01109::abilities()),
        act_01110::CODE => Some(act_01110::abilities()),
        agenda_01105::CODE => Some(agenda_01105::abilities()),
        agenda_01106::CODE => Some(agenda_01106::abilities()),
        agenda_01107::CODE => Some(agenda_01107::abilities()),
        attic::CODE => Some(attic::abilities()),
        automatic_45::CODE => Some(automatic_45::abilities()),
        cellar::CODE => Some(cellar::abilities()),
        deduction::CODE => Some(deduction::abilities()),
        dr_milan_christopher::CODE => Some(dr_milan_christopher::abilities()),
        emergency_cache::CODE => Some(emergency_cache::abilities()),
        guard_dog::CODE => Some(guard_dog::abilities()),
        guts::CODE => Some(guts::abilities()),
        holy_rosary::CODE => Some(holy_rosary::abilities()),
        hyperawareness::CODE => Some(hyperawareness::abilities()),
        machete::CODE => Some(machete::abilities()),
        magnifying_glass::CODE => Some(magnifying_glass::abilities()),
        manual_dexterity::CODE => Some(manual_dexterity::abilities()),
        overpower::CODE => Some(overpower::abilities()),
        perception::CODE => Some(perception::abilities()),
        physical_training::CODE => Some(physical_training::abilities()),
        roland_38_special::CODE => Some(roland_38_special::abilities()),
        roland_banks::CODE => Some(roland_banks::abilities()),
        treachery_01007::CODE => Some(treachery_01007::abilities()),
        treachery_01162::CODE => Some(treachery_01162::abilities()),
        treachery_01163::CODE => Some(treachery_01163::abilities()),
        treachery_01164::CODE => Some(treachery_01164::abilities()),
        treachery_01165::CODE => Some(treachery_01165::abilities()),
        treachery_01166::CODE => Some(treachery_01166::abilities()),
        treachery_01167::CODE => Some(treachery_01167::abilities()),
        treachery_01168::CODE => Some(treachery_01168::abilities()),
        unexpected_courage::CODE => Some(unexpected_courage::abilities()),
        vicious_blow::CODE => Some(vicious_blow::abilities()),
        working_a_hunch::CODE => Some(working_a_hunch::abilities()),
        _ => None,
    }
}

/// Resolve an [`Effect::Native`](card_dsl::dsl::Effect::Native) tag to the
/// card-local Rust fn that implements it. Mirrors [`abilities_for`]'s
/// per-card delegation; returns `None` for unregistered tags.
#[must_use]
pub fn native_effect_for(tag: &str) -> Option<game_core::card_registry::NativeEffectFn> {
    act_01108::native_effect_for(tag)
        .or_else(|| act_01109::native_effect_for(tag))
        .or_else(|| agenda_01105::native_effect_for(tag))
        .or_else(|| agenda_01106::native_effect_for(tag))
        .or_else(|| agenda_01107::native_effect_for(tag))
        .or_else(|| guard_dog::native_effect_for(tag))
        .or_else(|| treachery_01007::native_effect_for(tag))
        .or_else(|| treachery_01166::native_effect_for(tag))
        .or_else(|| treachery_01167::native_effect_for(tag))
        .or_else(|| treachery_01168::native_effect_for(tag))
}
