//! Static card metadata types.
//!
//! These types describe a card as printed: code, name, class, type,
//! cost, traits, skill icons, etc. They live in `card-dsl` so both
//! sides of the engine-corpus boundary can construct and consume them
//! without one depending on the other: the engine (in `game-core`)
//! queries metadata when resolving actions — e.g. `PlayCard` reads
//! [`CardMetadata::card_type`] to choose where the played card lands
//! — while the `cards` crate populates the corpus (generated from the
//! pinned `ArkhamDB` snapshot) and installs it via
//! `game_core::card_registry`.
//!
//! Card *effect logic* (hand-implemented abilities) is separate; it's
//! looked up through the registry too but lives in
//! [`crate::dsl::Ability`].

use crate::dsl::Stat;
use serde::{Deserialize, Serialize};

/// Investigator class. Translation of upstream's `faction_code` field
/// to the rulebook's preferred term.
///
/// `Mythos` is used for encounter-set cards (treacheries, enemies,
/// scenario-specific things). `Weakness` covers basic weaknesses; per-
/// investigator weaknesses are encoded as story assets / treacheries
/// with a regular class plus a `weakness` subtype, not via this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Class {
    Guardian,
    Seeker,
    Rogue,
    Mystic,
    Survivor,
    Neutral,
    Mythos,
}

/// Top-level card type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CardType {
    Investigator,
    Asset,
    Event,
    Skill,
    Treachery,
    Enemy,
    Location,
    Agenda,
    Act,
    Scenario,
    Story,
}

/// An equipment slot occupied by an asset in play.
///
/// Multi-slot items (e.g. two-handed weapons) appear in
/// [`CardMetadata::slots`] as multiple entries of the same variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Slot {
    Hand,
    Accessory,
    Ally,
    Arcane,
    Body,
    Tarot,
}

/// Skill icons printed on a card. Contributed to a skill test's total
/// when the card is committed to that test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillIcons {
    pub willpower: u8,
    pub intellect: u8,
    pub combat: u8,
    pub agility: u8,
    /// Wild icons match any skill in a skill test.
    pub wild: u8,
}

/// Where on the location map an encounter enemy spawns.
///
/// Phase-4 minimal set: just a printed location code. Future variants
/// (`LeadInvestigator`, `LowestSanityInvestigator`, `NearestUnexplored`,
/// etc.) land with the first Phase-7+ card that needs them.
///
/// **Why a [`String`] code rather than a `LocationCode` newtype.**
/// Locations in Arkham are cards with `ArkhamDB` codes; the namespace
/// is shared at the data level. Introducing a distinct
/// `LocationCode` newtype would block accidental cross-use at the
/// engine level without a concrete consumer asking for that
/// distinction. Reuse `CardCode` (which is a [`String`] newtype in
/// `game-core::state::card`) by passing the bare string here; the
/// engine's spawn handler wraps it on lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpawnLocation {
    /// Fixed-location spawn — the named location's printed code.
    Specific(String),
}

/// Spawn rule for an encounter-deck enemy.
///
/// `None` on [`CardMetadata::spawn`] means "no spawn instruction" — per
/// Rules Reference p.24, the enemy spawns engaged with the drawing
/// investigator, placed in that investigator's threat area.
///
/// **Why a nested struct, not flat fields on `CardMetadata`.** So
/// spawn-related fields can grow (e.g. `engagement:
/// EngagementOnSpawn` for Aloof / "spawn unengaged" cards,
/// `also_spawn_doom_at: ...` for the rare multi-effect spawns)
/// without churning every enemy declaration in the generated corpus.
/// Phase-4 ships only `location`; later variants land alongside the
/// cards that force them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Spawn {
    /// Where the enemy spawns.
    pub location: SpawnLocation,
}

/// An enemy's prey instruction (Rules Reference p.17): which
/// investigator it pursues / engages when it has a choice.
///
/// Phase-4 ships `Default` + `HighestStat`. `Default` covers "no prey
/// instruction" and "Prey – nearest" — among equidistant / co-located
/// investigators all are equal, so the lead investigator breaks the tie
/// (p.12 / p.17). `HighestStat(Stat::Combat)` is Ghoul Priest's
/// `Prey – Highest [combat]`. Other printed variants (`Lowest`,
/// `Bearer only`, `Most clues`, …) land with their first card consumer;
/// `#[non_exhaustive]` keeps that additive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Prey {
    /// No discriminating instruction — all candidates are equal; the
    /// lead investigator breaks ties.
    #[default]
    Default,
    /// Pursue / engage the investigator with the highest value of the
    /// given stat; ties fall to the lead investigator.
    HighestStat(Stat),
}

/// Static metadata for one card as printed.
///
/// This is the universal shape; type-specific data (location shroud,
/// enemy stats, agenda doom thresholds, etc.) will land in dedicated
/// types in later phases. For Phase 2 the universal fields are enough.
///
/// Construction sites live in the `cards` crate (the pipeline-generated
/// corpus); the struct deliberately isn't `#[non_exhaustive]` so
/// generated code can use a struct literal. Adding a field requires
/// regenerating the corpus, which is the pipeline's job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardMetadata {
    /// Five-character `ArkhamDB` code (e.g. `"01059"`).
    pub code: String,
    /// Display name.
    pub name: String,
    /// Investigator class.
    pub class: Class,
    /// Top-level card type.
    pub card_type: CardType,
    /// Resource cost to play. `None` for skill cards, investigators,
    /// scenario cards, and a handful of cards with X-cost.
    pub cost: Option<i8>,
    /// XP cost in deckbuilding. `None` for cards that can't be added
    /// at deckbuilding time (encounter cards, scenario cards).
    pub xp: Option<u8>,
    /// Card text (game rules text), as printed.
    pub text: Option<String>,
    /// Flavor text, as printed.
    pub flavor: Option<String>,
    /// Illustrator credit.
    pub illustrator: Option<String>,
    /// Traits (Item, Tool, Insight, …). Parsed from upstream's period-
    /// delimited string into a clean list.
    pub traits: Vec<String>,
    /// Slots occupied while in play. Empty for non-asset cards or
    /// assets without slots.
    pub slots: Vec<Slot>,
    /// Skill icons committed when this card is committed to a test.
    pub skill_icons: SkillIcons,
    /// Maximum health. Applies to assets (allies) and enemies.
    pub health: Option<u8>,
    /// Maximum sanity. Applies to assets (allies).
    pub sanity: Option<u8>,
    /// Maximum copies of this card per deck during deckbuilding.
    pub deck_limit: u8,
    /// Number of copies of this card per box (printing run quantity,
    /// not deckbuilding limit).
    pub quantity: u8,
    /// Pack code this card belongs to (e.g. `"core"`, `"dwl"`).
    pub pack_code: String,
    /// 1-based card position within the pack.
    pub position: u32,
    /// True if the card text begins with a "Fast." paragraph — i.e.
    /// the card may be played as a Fast action, outside the normal
    /// Investigation-phase + active-investigator timing. Detected by
    /// the card-data-pipeline from raw `text` ("Fast." paragraph
    /// prefix). Phase-3 / Phase-4 scope: only asset and event cards
    /// can carry Fast (skill and treachery use is irrelevant to
    /// `PlayCard`); the field is populated on every card for
    /// uniformity. See `engine::dispatch::play_card` for the gate it
    /// drives.
    pub is_fast: bool,
    /// Spawn rule for encounter-deck enemies. `None` for enemies
    /// that don't spawn from the encounter deck (placed at scenario
    /// setup directly), for non-enemy card types, and as the
    /// pipeline's default for all generated entries until Phase-7's
    /// structured-spawn-text parsing lands.
    pub spawn: Option<Spawn>,
    /// Surge keyword (Rules Reference p.19). When `true`, after the
    /// card is drawn and resolved during a Mythos encounter draw, the
    /// drawing investigator immediately draws another encounter card.
    /// The pipeline emits `false` for every card until the first
    /// Phase-7+ scenario with a real surge-bearing card forces the
    /// pipeline-update work; the synthetic fixture sets `true`
    /// on its surge-bearing treachery to exercise the engine path.
    pub surge: bool,
    /// Peril keyword (Rules Reference p.18, referenced in p.24 1.4
    /// step 2). When `true`, the drawing investigator cannot confer
    /// and other players cannot play cards / trigger abilities /
    /// commit to that investigator's skill tests during resolution.
    /// Enforcement is not yet wired — no machinery exists for
    /// cross-investigator commit blocking. The field exists so cards
    /// can carry the keyword and the engine's step-2 call site can
    /// become load-bearing when the enforcement PR lands.
    pub peril: bool,
}

#[cfg(test)]
mod is_fast_tests {
    use super::*;

    #[test]
    fn metadata_serde_roundtrip_preserves_is_fast() {
        let original = CardMetadata {
            code: "01030".into(),
            name: "Magnifying Glass".into(),
            class: Class::Seeker,
            card_type: CardType::Asset,
            cost: Some(1),
            xp: Some(0),
            text: Some("Fast.\nYou get +1 [intellect] while investigating.".into()),
            flavor: None,
            illustrator: None,
            traits: vec!["Item".into(), "Tool".into()],
            slots: vec![Slot::Hand],
            skill_icons: SkillIcons::default(),
            health: None,
            sanity: None,
            deck_limit: 2,
            quantity: 1,
            pack_code: "core".into(),
            position: 30,
            is_fast: true,
            spawn: None,
            surge: false,
            peril: false,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(back.is_fast);
    }

    #[test]
    fn card_metadata_serde_roundtrip_preserves_surge_and_peril() {
        let original = CardMetadata {
            code: "_synth_surge_treachery".into(),
            name: "Synth Surge Treachery".into(),
            class: Class::Mythos,
            card_type: CardType::Treachery,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: SkillIcons::default(),
            health: None,
            sanity: None,
            deck_limit: 1,
            quantity: 1,
            pack_code: "_synth".into(),
            position: 1,
            is_fast: false,
            spawn: None,
            surge: true,
            peril: false,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(back.surge);
        assert!(!back.peril);
    }
}

#[cfg(test)]
mod prey_tests {
    use super::*;

    #[test]
    fn prey_default_is_default() {
        assert_eq!(Prey::default(), Prey::Default);
    }

    #[test]
    fn prey_serde_roundtrip_highest_stat() {
        let original = Prey::HighestStat(Stat::Combat);
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Prey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    #[test]
    fn spawn_specific_round_trips_through_serde_json() {
        let original = Spawn {
            location: SpawnLocation::Specific("01112".to_owned()),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let recovered: Spawn = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, recovered);
    }

    #[test]
    fn card_metadata_serde_roundtrip_preserves_spawn_specific() {
        let original = CardMetadata {
            code: "_synth_enemy".into(),
            name: "Synth Enemy".into(),
            class: Class::Mythos,
            card_type: CardType::Enemy,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: SkillIcons::default(),
            health: Some(1),
            sanity: None,
            deck_limit: 1,
            quantity: 1,
            pack_code: "_synth".into(),
            position: 1,
            is_fast: false,
            spawn: Some(Spawn {
                location: SpawnLocation::Specific("_synth_loc".into()),
            }),
            surge: false,
            peril: false,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn card_metadata_serde_roundtrip_preserves_spawn_none() {
        let original = CardMetadata {
            code: "01000".into(),
            name: "Random Basic Weakness".into(),
            class: Class::Neutral,
            card_type: CardType::Treachery,
            cost: None,
            xp: None,
            text: None,
            flavor: None,
            illustrator: None,
            traits: Vec::new(),
            slots: Vec::new(),
            skill_icons: SkillIcons::default(),
            health: None,
            sanity: None,
            deck_limit: 0,
            quantity: 1,
            pack_code: "core".into(),
            position: 0,
            is_fast: false,
            spawn: None,
            surge: false,
            peril: false,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(back.spawn.is_none());
    }
}
