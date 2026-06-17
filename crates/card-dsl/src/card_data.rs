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
/// Multi-slot items (e.g. two-handed weapons) appear in an asset's
/// [`CardKind::Asset`] `slots` as multiple entries of the same variant.
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

/// The four base skill values.
///
/// Deliberately NOT `#[non_exhaustive]`: the four skills are fixed by
/// FFG's rules. Card effects modify these values at query time; they
/// don't add new fields. Pure data — `game-core` re-exports it at
/// `game_core::state::Skills`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skills {
    /// Used for tests against effects of the will / fear.
    pub willpower: i8,
    /// Used for investigate tests.
    pub intellect: i8,
    /// Used for fight tests.
    pub combat: i8,
    /// Used for evade tests.
    pub agility: i8,
}

impl Skills {
    /// Lookup the value for a given [`SkillKind`].
    #[must_use]
    pub fn value(&self, kind: SkillKind) -> i8 {
        match kind {
            SkillKind::Willpower => self.willpower,
            SkillKind::Intellect => self.intellect,
            SkillKind::Combat => self.combat,
            SkillKind::Agility => self.agility,
        }
    }
}

/// Which of the four skill values a skill test is being made against.
///
/// Deliberately NOT `#[non_exhaustive]` — same rationale as [`Skills`]:
/// the four skill kinds are fixed by FFG's rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillKind {
    /// Tests against the will, fear, sanity-eroding effects.
    Willpower,
    /// Tests for investigating, deduction, lore.
    Intellect,
    /// Tests for fighting, combat, physical strength.
    Combat,
    /// Tests for evading, dexterity, speed.
    Agility,
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
/// `None` on [`CardKind::Enemy`]'s `spawn` means "no spawn instruction" — per
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
/// `Default` covers "no prey instruction" and "Prey – nearest" — among
/// equidistant / co-located investigators all are equal, so the lead
/// investigator breaks the tie (p.12 / p.17). [`Ranked`](Self::Ranked)
/// covers every *comparative* prey line as a `{ direction, measure }`
/// pair: Ghoul Priest's `Highest [combat]` (`Highest` +
/// `Skill(Combat)`) and Ravenous Ghoul's "Lowest remaining health"
/// (`Lowest` + `RemainingHealth`). `#[non_exhaustive]` leaves room for
/// genuinely non-comparative future shapes (e.g. "Bearer only").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Prey {
    /// No discriminating instruction — all candidates are equal; the
    /// lead investigator breaks ties.
    #[default]
    Default,
    /// Pursue / engage the investigator with the highest or lowest value
    /// of `measure`; ties fall to the lead investigator.
    Ranked {
        /// Whether the highest or lowest measure value is preferred.
        direction: PreyDirection,
        /// The quantity investigators are ranked by.
        measure: PreyMeasure,
    },
}

/// Whether a [`Prey::Ranked`] instruction prefers the highest or lowest
/// value of its [`PreyMeasure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PreyDirection {
    /// Prefer the investigator with the greatest measure value.
    Highest,
    /// Prefer the investigator with the least measure value.
    Lowest,
}

/// The quantity a [`Prey::Ranked`] instruction ranks investigators by.
///
/// Exhaustive (unlike [`Prey`]): adding a measure must force the engine's
/// `resolve_prey` to wire it, so the compiler flags the site. New printed
/// measures land here with their first card consumer — `RemainingSanity`
/// (Lowest remaining sanity), `Clues` (Most clues), `CardsInHand` (Fewest
/// cards in hand), ….
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PreyMeasure {
    /// One of the four skills (Rules Reference p.17). Ghoul Priest's
    /// `Highest [combat]` is `Skill(SkillKind::Combat)`.
    Skill(SkillKind),
    /// Remaining health = base health − damage (Rules Reference p.12).
    /// Ravenous Ghoul's "Lowest remaining health".
    RemainingHealth,
}

/// Static metadata for one card as printed: an identity core shared by
/// every card, plus type-specific data in [`kind`](CardMetadata::kind).
///
/// Construction sites live in the `cards` crate (the pipeline-generated
/// corpus) and in mocks; deliberately NOT `#[non_exhaustive]` so those
/// downstream crates can use a struct literal. Adding a field requires
/// regenerating the corpus, which is the pipeline's job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardMetadata {
    /// Five-character `ArkhamDB` code (e.g. `"01059"`). Identity and the
    /// registry's binary-search / sort key.
    pub code: String,
    /// Display name.
    pub name: String,
    /// Traits (Item, Tool, Ghoul, …). Empty when the card has none.
    pub traits: Vec<String>,
    /// Card text (game rules text), as printed.
    pub text: Option<String>,
    /// Pack code this card belongs to (e.g. `"core"`, `"dwl"`).
    pub pack_code: String,
    /// Type-specific data.
    pub kind: CardKind,
}

/// A location's printed clue value. `PerInvestigator(n)` places
/// `n × (number of investigators who started the scenario)` on reveal;
/// `Fixed(n)` places exactly `n`. Distinguishes `ArkhamDB`'s `clues_fixed`
/// (absent/false → per-investigator; `true` → fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClueValue {
    /// `value × #investigators` at reveal time.
    PerInvestigator(u8),
    /// Exactly `value`, regardless of investigator count.
    Fixed(u8),
}

/// An enemy's printed health. Mirrors [`ClueValue`]: `PerInvestigator(n)`
/// scales by the number of investigators in the game (Rules Reference
/// p.12); `Fixed(n)` is a flat value. Distinguishes `ArkhamDB`'s
/// `health_per_investigator` (absent/false → fixed; `true` →
/// per-investigator). Note the polarity is the opposite of [`ClueValue`],
/// whose clues default to per-investigator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthValue {
    /// Exactly `value`, regardless of investigator count.
    Fixed(u8),
    /// `value × #investigators` at spawn time.
    PerInvestigator(u8),
}

/// Limited-use tokens an asset enters play with ("Uses (4 ammo)").
/// Spending them is a [`Cost::SpendUses`](crate::dsl::Cost::SpendUses);
/// depletion blocks the ability that pays in them. Pipeline-parsed from
/// card text. The engine's runtime uses-pool is seeded from this on
/// enter-play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Uses {
    /// What the tokens are called on the card.
    pub kind: UseKind,
    /// How many the asset enters play with.
    pub count: u8,
    /// Whether the card's text discards it when these uses deplete ("If First
    /// Aid has no supplies, discard it.", RR p.27). `false` for uses-assets
    /// that stay in play when empty (Flashlight, weapons). Pipeline-parsed.
    pub discard_when_empty: bool,
}

/// A named-uses kind for asset cards that track a finite resource.
///
/// Translation of the rulebook's typed-uses taxonomy. Cards declare
/// what flavor of uses they have ("Uses (3 charges)", "Uses (1 ammo)")
/// and effects spend them with a [`Cost::SpendUses`](crate::dsl::Cost::SpendUses).
///
/// Lives here in `card-dsl` (the lowest layer) so both the printed
/// metadata ([`Uses`]) and the engine's runtime pool key off one type;
/// `game_core::state` re-exports it at the historical path.
///
/// Phase-3 minimal set; cards using exotic uses (Time on some Dunwich
/// cards, Resource on a few Mystic effects) add their variant when
/// they land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UseKind {
    /// Charges — most spell assets (Rite of Seeking, Shrivelling).
    Charges,
    /// Ammo — firearms (.38 Special, .45 Automatic).
    Ammo,
    /// Secrets — Seeker investigation aids (Encyclopedia, Old Book of
    /// Lore in some cycles).
    Secrets,
    /// Supplies — Survivor tools (First Aid in some cycles, expedition
    /// caches).
    Supplies,
}

/// Per-card-type data. The discriminant mirrors [`CardType`] — read it
/// via [`CardMetadata::card_type`]. Player variants carry a [`Class`];
/// encounter variants do not (encounter cards have no player class).
///
/// Location / Act / Agenda variants and the `Enemy` combat stats land
/// with encounter-card ingestion (issue #252); this is the current
/// corpus's six types. Not `#[non_exhaustive]` for the same reason as
/// [`CardMetadata`] — the generated corpus constructs these variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardKind {
    /// Investigator — the player character; never deckbuilt.
    Investigator {
        /// Investigator class.
        class: Class,
        /// Base willpower / intellect / combat / agility.
        skills: Skills,
        /// Starting maximum health.
        health: u8,
        /// Starting maximum sanity.
        sanity: u8,
    },
    /// Asset — played to a play area; allies may hold health/sanity soak.
    Asset {
        /// Card class.
        class: Class,
        /// Resource cost to play (`None` for X-cost).
        cost: Option<i8>,
        /// XP cost in deckbuilding.
        xp: Option<u8>,
        /// Slots occupied while in play.
        slots: Vec<Slot>,
        /// Maximum health soak (allies).
        health: Option<u8>,
        /// Maximum sanity soak (allies).
        sanity: Option<u8>,
        /// Skill icons committed when this card is committed to a test.
        skill_icons: SkillIcons,
        /// Whether the card may be played as a Fast action.
        is_fast: bool,
        /// Maximum copies per deck.
        deck_limit: u8,
        /// Limited-use tokens granted on enter-play ("Uses (N ammo)"),
        /// or `None`. Pipeline-parsed from card text.
        uses: Option<Uses>,
    },
    /// Event — played from hand, then discarded.
    Event {
        /// Card class.
        class: Class,
        /// Resource cost to play.
        cost: Option<i8>,
        /// XP cost in deckbuilding.
        xp: Option<u8>,
        /// Skill icons committed when this card is committed to a test.
        skill_icons: SkillIcons,
        /// Whether the card may be played as a Fast action.
        is_fast: bool,
        /// Maximum copies per deck.
        deck_limit: u8,
    },
    /// Skill — committed to a skill test (never played for a cost).
    Skill {
        /// Card class.
        class: Class,
        /// XP cost in deckbuilding.
        xp: Option<u8>,
        /// Skill icons contributed when committed.
        skill_icons: SkillIcons,
        /// Maximum copies per deck.
        deck_limit: u8,
        /// "Max N committed per skill test" cap (Guts/Perception/… are `1`);
        /// `None` when uncapped. Enforced at the commit window.
        commit_limit: Option<u8>,
    },
    /// Enemy — an encounter (or weakness) creature.
    Enemy {
        /// Fight (combat difficulty).
        fight: u8,
        /// Evade difficulty.
        evade: u8,
        /// Damage dealt to an investigator on attack.
        damage: u8,
        /// Horror dealt to an investigator on attack.
        horror: u8,
        /// Maximum health (per-investigator or fixed).
        health: Option<HealthValue>,
        /// Victory points awarded when defeated (in the victory display).
        victory: Option<u8>,
        /// Spawn rule (`None` = default: engaged with the drawing
        /// investigator, Rules Reference p.24).
        spawn: Option<Spawn>,
        /// Surge keyword (Rules Reference p.19).
        surge: bool,
        /// Peril keyword (Rules Reference p.18).
        peril: bool,
        /// Hunter keyword (Rules Reference p.12).
        hunter: bool,
        /// Retaliate keyword (Rules Reference p.18).
        retaliate: bool,
        /// Prey instruction (Rules Reference p.17); `Prey::Default` when
        /// the card prints no prey line.
        prey: Prey,
        /// Copies of this card in the encounter deck (build multiplicity).
        quantity: u8,
    },
    /// Treachery — a one-shot encounter card resolved on reveal.
    Treachery {
        /// Surge keyword (Rules Reference p.19).
        surge: bool,
        /// Peril keyword (Rules Reference p.18).
        peril: bool,
        /// Copies of this card in the encounter deck (build multiplicity).
        quantity: u8,
    },
    /// Location — a place investigators move between and investigate.
    Location {
        /// Shroud (investigate difficulty).
        shroud: u8,
        /// Printed clue value (per-investigator or fixed).
        printed_clues: ClueValue,
        /// Victory points when in the victory display.
        victory: Option<u8>,
    },
    /// Act — the investigators' side of the act/agenda deck.
    Act {
        /// Clues the group spends to advance, or `None` for acts that
        /// advance on a non-clue objective.
        clue_threshold: Option<u8>,
        /// Victory points, if any.
        victory: Option<u8>,
    },
    /// Agenda — the doom side of the act/agenda deck.
    Agenda {
        /// Doom in play required to advance.
        doom_threshold: u8,
    },
}

impl CardMetadata {
    /// The card's [`CardType`] discriminant, derived from
    /// [`kind`](Self::kind).
    #[must_use]
    pub fn card_type(&self) -> CardType {
        match self.kind {
            CardKind::Investigator { .. } => CardType::Investigator,
            CardKind::Asset { .. } => CardType::Asset,
            CardKind::Event { .. } => CardType::Event,
            CardKind::Skill { .. } => CardType::Skill,
            CardKind::Enemy { .. } => CardType::Enemy,
            CardKind::Treachery { .. } => CardType::Treachery,
            CardKind::Location { .. } => CardType::Location,
            CardKind::Act { .. } => CardType::Act,
            CardKind::Agenda { .. } => CardType::Agenda,
        }
    }

    /// The player [`Class`], or `None` for encounter cards (which have
    /// no player class).
    #[must_use]
    pub fn class(&self) -> Option<Class> {
        match &self.kind {
            CardKind::Investigator { class, .. }
            | CardKind::Asset { class, .. }
            | CardKind::Event { class, .. }
            | CardKind::Skill { class, .. } => Some(*class),
            CardKind::Enemy { .. }
            | CardKind::Treachery { .. }
            | CardKind::Location { .. }
            | CardKind::Act { .. }
            | CardKind::Agenda { .. } => None,
        }
    }

    /// Skill icons contributed when this card is committed to a skill
    /// test. Player commit-cards (Asset/Event/Skill) carry them; every
    /// other kind contributes none (the default, all-zero icons).
    #[must_use]
    pub fn skill_icons(&self) -> SkillIcons {
        match &self.kind {
            CardKind::Asset { skill_icons, .. }
            | CardKind::Event { skill_icons, .. }
            | CardKind::Skill { skill_icons, .. } => *skill_icons,
            CardKind::Investigator { .. }
            | CardKind::Enemy { .. }
            | CardKind::Treachery { .. }
            | CardKind::Location { .. }
            | CardKind::Act { .. }
            | CardKind::Agenda { .. } => SkillIcons::default(),
        }
    }

    /// Whether the card may be played as a Fast action. Only Asset and
    /// Event cards can; everything else is `false`.
    #[must_use]
    pub fn is_fast(&self) -> bool {
        matches!(
            self.kind,
            CardKind::Asset { is_fast: true, .. } | CardKind::Event { is_fast: true, .. }
        )
    }

    /// Surge keyword (Rules Reference p.19). Only Enemy/Treachery
    /// encounter cards carry it; everything else is `false`.
    #[must_use]
    pub fn surge(&self) -> bool {
        matches!(
            self.kind,
            CardKind::Enemy { surge: true, .. } | CardKind::Treachery { surge: true, .. }
        )
    }

    /// Peril keyword (Rules Reference p.18). Only Enemy/Treachery
    /// encounter cards carry it; everything else is `false`.
    #[must_use]
    pub fn peril(&self) -> bool {
        matches!(
            self.kind,
            CardKind::Enemy { peril: true, .. } | CardKind::Treachery { peril: true, .. }
        )
    }
}

#[cfg(test)]
mod skills_tests {
    use super::{SkillKind, Skills};

    #[test]
    fn skills_value_indexes_each_kind() {
        let s = Skills {
            willpower: 3,
            intellect: 2,
            combat: 4,
            agility: 1,
        };
        assert_eq!(s.value(SkillKind::Willpower), 3);
        assert_eq!(s.value(SkillKind::Intellect), 2);
        assert_eq!(s.value(SkillKind::Combat), 4);
        assert_eq!(s.value(SkillKind::Agility), 1);
    }
}

#[cfg(test)]
mod is_fast_tests {
    use super::*;

    #[test]
    fn metadata_serde_roundtrip_preserves_is_fast() {
        let original = CardMetadata {
            code: "01030".into(),
            name: "Magnifying Glass".into(),
            text: Some("Fast.\nYou get +1 [intellect] while investigating.".into()),
            traits: vec!["Item".into(), "Tool".into()],
            pack_code: "core".into(),
            kind: CardKind::Asset {
                class: Class::Seeker,
                cost: Some(1),
                xp: Some(0),
                slots: vec![Slot::Hand],
                health: None,
                sanity: None,
                skill_icons: SkillIcons::default(),
                is_fast: true,
                deck_limit: 2,
                uses: None,
            },
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(matches!(back.kind, CardKind::Asset { is_fast: true, .. }));
    }

    #[test]
    fn asset_uses_round_trips() {
        let uses = Some(Uses {
            kind: UseKind::Ammo,
            count: 4,
            discard_when_empty: false,
        });
        let json = serde_json::to_string(&uses).expect("serialize");
        let back: Option<Uses> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, uses);
    }

    #[test]
    fn card_metadata_serde_roundtrip_preserves_surge_and_peril() {
        let original = CardMetadata {
            code: "_synth_surge_treachery".into(),
            name: "Synth Surge Treachery".into(),
            text: None,
            traits: Vec::new(),
            pack_code: "_synth".into(),
            kind: CardKind::Treachery {
                surge: true,
                peril: false,
                quantity: 1,
            },
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(matches!(
            back.kind,
            CardKind::Treachery {
                surge: true,
                peril: false,
                ..
            }
        ));
    }

    #[test]
    fn card_type_is_derived_from_kind() {
        let m = CardMetadata {
            code: "x".into(),
            name: "X".into(),
            traits: vec![],
            text: None,
            pack_code: "core".into(),
            kind: CardKind::Skill {
                class: Class::Seeker,
                xp: None,
                skill_icons: SkillIcons::default(),
                deck_limit: 2,
                commit_limit: None,
            },
        };
        assert_eq!(m.card_type(), CardType::Skill);
        assert_eq!(m.class(), Some(Class::Seeker));
    }

    #[test]
    fn encounter_cards_have_no_class() {
        let m = CardMetadata {
            code: "y".into(),
            name: "Y".into(),
            traits: vec![],
            text: None,
            pack_code: "core".into(),
            kind: CardKind::Treachery {
                surge: false,
                peril: false,
                quantity: 1,
            },
        };
        assert_eq!(m.card_type(), CardType::Treachery);
        assert_eq!(m.class(), None);
    }

    #[test]
    fn new_encounter_kinds_have_no_class_and_right_type() {
        let loc = CardMetadata {
            code: "01111".into(),
            name: "Study".into(),
            traits: vec![],
            text: None,
            pack_code: "core".into(),
            kind: CardKind::Location {
                shroud: 2,
                printed_clues: ClueValue::PerInvestigator(2),
                victory: None,
            },
        };
        assert_eq!(loc.card_type(), CardType::Location);
        assert_eq!(loc.class(), None);

        let agenda = CardMetadata {
            code: "01105".into(),
            name: "Agenda".into(),
            traits: vec![],
            text: None,
            pack_code: "core".into(),
            kind: CardKind::Agenda { doom_threshold: 3 },
        };
        assert_eq!(agenda.card_type(), CardType::Agenda);
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
    fn prey_serde_roundtrip_ranked_skill() {
        let original = Prey::Ranked {
            direction: PreyDirection::Highest,
            measure: PreyMeasure::Skill(SkillKind::Combat),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Prey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn prey_serde_roundtrip_ranked_remaining_health() {
        let original = Prey::Ranked {
            direction: PreyDirection::Lowest,
            measure: PreyMeasure::RemainingHealth,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Prey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }

    #[test]
    fn prey_serde_roundtrip_default() {
        let original = Prey::Default;
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Prey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
    }
}

#[cfg(test)]
mod clue_value_tests {
    use super::*;

    #[test]
    fn clue_value_round_trips_through_serde() {
        for cv in [ClueValue::PerInvestigator(2), ClueValue::Fixed(3)] {
            let json = serde_json::to_string(&cv).expect("serialize");
            let back: ClueValue = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(cv, back);
        }
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
    fn health_value_serde_roundtrip() {
        for hv in [HealthValue::Fixed(4), HealthValue::PerInvestigator(5)] {
            let json = serde_json::to_string(&hv).expect("serialize");
            assert_eq!(
                serde_json::from_str::<HealthValue>(&json).expect("deserialize"),
                hv
            );
        }
    }

    #[test]
    fn card_metadata_serde_roundtrip_preserves_spawn_specific() {
        let original = CardMetadata {
            code: "_synth_enemy".into(),
            name: "Synth Enemy".into(),
            text: None,
            traits: Vec::new(),
            pack_code: "_synth".into(),
            kind: CardKind::Enemy {
                fight: 3,
                evade: 2,
                damage: 1,
                horror: 1,
                health: Some(HealthValue::Fixed(1)),
                victory: None,
                spawn: Some(Spawn {
                    location: SpawnLocation::Specific("_synth_loc".into()),
                }),
                surge: false,
                peril: false,
                hunter: false,
                retaliate: false,
                prey: Prey::Default,
                quantity: 1,
            },
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
            text: None,
            traits: Vec::new(),
            pack_code: "core".into(),
            kind: CardKind::Treachery {
                surge: false,
                peril: false,
                quantity: 1,
            },
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: CardMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, original);
        assert!(matches!(back.kind, CardKind::Treachery { .. }));
    }
}
