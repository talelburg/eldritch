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
/// scenario-specific things). Weaknesses are NOT a variant here: both
/// basic and per-investigator weaknesses carry a regular class (usually
/// `Neutral`) plus the [`CardMetadata::weakness`] subtype flag.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

impl SkillIcons {
    /// How many icons printed here match `skill` itself, **excluding** wild.
    ///
    /// The two callers differ in what they do with the count — one sums the
    /// ST.5 contribution, one asks the ST.2 eligibility question below — so
    /// this returns the number rather than a bool.
    #[must_use]
    pub fn matching(self, skill: SkillKind) -> u8 {
        match skill {
            SkillKind::Willpower => self.willpower,
            SkillKind::Intellect => self.intellect,
            SkillKind::Combat => self.combat,
            SkillKind::Agility => self.agility,
        }
    }

    /// Whether a card printing these icons may be committed to a test of
    /// `skill`.
    ///
    /// Rules Reference Appendix II, ST.2: *"An appropriate skill icon is
    /// either one that matches the skill being tested, or a wild icon. […]
    /// Cards that lack an appropriate skill icon may not be committed to a
    /// skill test."*
    #[must_use]
    pub fn appropriate_for(self, skill: SkillKind) -> bool {
        self.matching(skill) > 0 || self.wild > 0
    }
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
/// Phase-4 minimal set: a printed location code, plus an explicit
/// [`Unrepresented`](Self::Unrepresented) marker for spawn clauses the
/// pipeline could not model. Future *modelled* variants (`AnyEmptyLocation`,
/// `FarthestFromYou`, `NearestWithTrait`, `MostClues`, `EngagedWithPrey`)
/// land with the PR that implements the spawning investigator's choice
/// among multiple valid locations.
///
/// **Why `Unrepresented` is a variant rather than `spawn: None`.**
/// `spawn: None` is the *positive* rule "this enemy prints no Spawn line",
/// which [`Spawn`] documents as "spawns engaged with the drawing
/// investigator". Folding an unparsed clause into it makes the engine apply
/// a different, valid rule to the wrong enemy — Acolyte 01169's "**Spawn** -
/// Any empty location." would spawn it at the one location guaranteed *not*
/// to be empty. Keeping the two apart lets the engine refuse loudly on
/// unmodelled content, the way `PlayCard` refuses an unimplemented card
/// (#635).
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
    /// The card prints a spawn instruction the pipeline could not model,
    /// carrying the clause as the pipeline parsed it (e.g. `"Any empty
    /// location"`) so the refusal names what is missing.
    ///
    /// Deliberately **not** exhaustive-match-escaping: every consumer must
    /// handle it, and adding a modelled variant later is a compile error at
    /// each site rather than a silent fallthrough.
    Unrepresented(String),
}

/// Spawn rule for an encounter-deck enemy.
///
/// `None` on [`CardKind::Enemy`]'s `spawn` means "no spawn instruction" — per
/// Rules Reference p.24, the enemy spawns engaged with the drawing
/// investigator, placed in that investigator's threat area. An enemy that
/// *does* print a Spawn line the pipeline could not model carries
/// `Some(Spawn { location: SpawnLocation::Unrepresented(clause) })`, never
/// `None`; see [`SpawnLocation::Unrepresented`].
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
    /// `ArkhamDB` code (e.g. `"01059"`). Usually five characters, but
    /// double-sided scenario cards use a letter-suffixed form for the
    /// second side (e.g. `"01121a"` / `"01121b"`) — don't assume fixed
    /// width. Identity and the registry's binary-search / sort key.
    pub code: String,
    /// Display name.
    pub name: String,
    /// Traits (Item, Tool, Ghoul, …). Empty when the card has none.
    pub traits: Vec<String>,
    /// Card text (game rules text), as printed.
    pub text: Option<String>,
    /// Reverse-side display name, verbatim from `ArkhamDB` `back_name`. The
    /// motivating case is the act/agenda "1b" face (e.g. `"A Lapse in Time"`),
    /// but the field carries whatever back name a double-sided card prints
    /// (also many locations); `None` when absent.
    pub back_name: Option<String>,
    /// Reverse-side text, verbatim from `ArkhamDB` `back_text`. For an
    /// act/agenda this is the on-advance effect on the "1b" face — but the
    /// field is generic (locations carry reverse rules; investigator backs
    /// carry the deckbuilding block), so a consumer keying off "advance
    /// effect" must first confirm the card is an act/agenda. `None` when
    /// absent.
    pub back_text: Option<String>,
    /// Pack code this card belongs to (e.g. `"core"`, `"dwl"`).
    pub pack_code: String,
    /// Whether this card is a weakness (`ArkhamDB` subtype `"weakness"` or
    /// `"basicweakness"`). Orthogonal to card type — a weakness may be a
    /// Treachery, Enemy, or Asset.
    pub weakness: bool,
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
        /// Whether the card prints "Play only during your turn" (restricts a
        /// Fast play to the controller's own Investigation turn). Pipeline-
        /// parsed, like `is_fast`.
        play_only_during_turn: bool,
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
        /// Whether the card prints "Play only during your turn" (restricts a
        /// Fast play to the controller's own Investigation turn). Pipeline-
        /// parsed, like `is_fast`.
        play_only_during_turn: bool,
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

    /// Whether the card is restricted to "Play only during your turn" (Mind
    /// over Matter 01036, Working a Hunch 01037, …). Only Asset/Event carry
    /// it; everything else is `false`. Mirrors [`is_fast`](Self::is_fast).
    #[must_use]
    pub fn play_only_during_turn(&self) -> bool {
        matches!(
            self.kind,
            CardKind::Asset {
                play_only_during_turn: true,
                ..
            } | CardKind::Event {
                play_only_during_turn: true,
                ..
            }
        )
    }

    /// The printed resource cost to play this card from hand (Rules Reference
    /// p.7, "Costs"). `Some(n)` with `n >= 0` is a fixed cost.
    ///
    /// The two other shapes are distinguishable, and the play path relies on
    /// it: a printed **X** cost arrives as `Some(-2)` — `ArkhamDB`'s sentinel,
    /// carried through the pipeline unchanged — while a printed **`"–"`**
    /// cost (which includes every permanent) arrives as `None`, as does any
    /// card type that is never played for a cost (Skill, encounter cards, …).
    /// The distinction matters because the rules treat them differently: a
    /// `"–"` card *cannot be played at all*, whereas an X cost is a real cost
    /// with a player-chosen amount. See `check_play_resource_cost_payable`.
    #[must_use]
    pub fn play_cost(&self) -> Option<i8> {
        match self.kind {
            CardKind::Asset { cost, .. } | CardKind::Event { cost, .. } => cost,
            CardKind::Investigator { .. }
            | CardKind::Skill { .. }
            | CardKind::Enemy { .. }
            | CardKind::Treachery { .. }
            | CardKind::Location { .. }
            | CardKind::Act { .. }
            | CardKind::Agenda { .. } => None,
        }
    }

    /// The equipment slots this card occupies while in play (Rules Reference
    /// p.19). Only `Asset` cards carry slots; every other kind occupies none
    /// (the empty slice). A slot-less asset (`Vec::new()`) also returns empty
    /// — there is no limit on slot-less assets in play.
    #[must_use]
    pub fn slots(&self) -> &[Slot] {
        match &self.kind {
            CardKind::Asset { slots, .. } => slots,
            CardKind::Investigator { .. }
            | CardKind::Event { .. }
            | CardKind::Skill { .. }
            | CardKind::Enemy { .. }
            | CardKind::Treachery { .. }
            | CardKind::Location { .. }
            | CardKind::Act { .. }
            | CardKind::Agenda { .. } => &[],
        }
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

    /// Whether this card is a weakness. Mirrors the stored
    /// [`weakness`](Self::weakness) field.
    #[must_use]
    pub fn is_weakness(&self) -> bool {
        self.weakness
    }
}

#[cfg(test)]
mod tests;
