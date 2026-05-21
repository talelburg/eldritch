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
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(back.is_fast);
    }
}
