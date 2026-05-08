//! Static card metadata types.
//!
//! These types describe a card as printed: code, name, class, type,
//! cost, traits, skill icons, etc. They are what the
//! [`card-data-pipeline`] CLI emits into [`crate::generated`] and what
//! the rest of the engine queries when it needs to know "what *is*
//! this card." They do **not** describe a card's *effect logic* —
//! that lives in [`crate::impls`].
//!
//! [`card-data-pipeline`]: ../../card_data_pipeline/index.html

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
#[non_exhaustive]
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
#[non_exhaustive]
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

/// Static metadata for one card as printed.
///
/// This is the universal shape; type-specific data (location shroud,
/// enemy stats, agenda doom thresholds, etc.) will land in dedicated
/// types in later phases. For Phase 2 the universal fields are enough.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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
}
