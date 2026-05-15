//! Card identifiers and per-instance in-play state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `ArkhamDB` card code (e.g. `"01030"` for Magnifying Glass).
///
/// Newtype over `String` so we can't accidentally pass arbitrary strings
/// where a card code is expected. Serializes transparently as a string
/// so the wire format matches the upstream JSON.
///
/// The cards crate's lookups (`cards::by_code`, `cards::abilities_for`)
/// take `&str`; deref or call `.as_str()` to bridge.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardCode(pub String);

impl CardCode {
    /// Wrap a string into a [`CardCode`].
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the inner string. Use this to bridge to `&str`-taking APIs
    /// like `cards::by_code(code.as_str())`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CardCode {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for CardCode {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for CardCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// A card-bearing zone, used as the `from` field on movement events
/// (e.g. [`Event::CardDiscarded`](crate::Event::CardDiscarded)).
///
/// Phase-3 minimal set. Discard is a destination but never a `from`
/// in the current event set; encounter / weakness / out-of-game zones
/// land when they're needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Zone {
    /// A player's hand.
    Hand,
    /// A player's deck (top or bottom — events that need the
    /// distinction can record it separately).
    Deck,
    /// Cards currently in play under an investigator's control.
    InPlay,
}

/// Unique identifier for a specific copy of a card in play.
///
/// Assigned by the engine when a card enters play (via the per-state
/// [`next_card_instance_id`](crate::state::GameState::next_card_instance_id)
/// counter). Lets card effects target a specific copy when an
/// investigator has multiple of the same card in play (e.g. two
/// copies of Magnifying Glass — exhausting one shouldn't exhaust the
/// other).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardInstanceId(pub u32);

/// A named-uses kind for asset cards that track a finite resource.
///
/// Translation of the rulebook's typed-uses taxonomy. Cards declare
/// what flavor of uses they have ("Uses (3 charges)", "Uses (1 ammo)")
/// and effects spend them with primitives that take a [`UseKind`].
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

/// State for a single copy of a card in play under an investigator's
/// control.
///
/// Replaces the bare [`CardCode`] entries that lived in
/// [`Investigator::cards_in_play`](crate::state::Investigator::cards_in_play)
/// through Phase-3. Carries the per-instance fields that activated
/// abilities, soak effects, and identity-aware queries need:
///
/// - `instance_id` so effects can name a specific copy.
/// - `exhausted` for ready/exhaust dispatch.
/// - `uses` for typed named-uses tracking (charges, ammo, …).
/// - `accumulated_damage` / `accumulated_horror` for asset soak
///   (the soak mechanic itself is #44; this struct just owns the
///   storage).
///
/// Construction goes through [`CardInPlay::enter_play`] so all the
/// defaults flow from one place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CardInPlay {
    /// Which card this instance represents.
    pub code: CardCode,
    /// Per-instance identifier; unique within a scenario.
    pub instance_id: CardInstanceId,
    /// Whether the card is currently exhausted (turned 90°). Blocks
    /// activated abilities (which un-ready on activation); does
    /// **not** block passive constant modifiers — those apply
    /// regardless of ready/exhaust state per the rules.
    pub exhausted: bool,
    /// Named-uses pool: kind → remaining count. Empty if the card has
    /// no typed uses. A card can in principle carry multiple kinds at
    /// once (rare); each kind has its own counter.
    pub uses: BTreeMap<UseKind, u8>,
    /// Damage accumulated on this asset (for damage-soak / ally
    /// health). Distinct from the controlling investigator's damage
    /// — assets soak damage on themselves until they discard.
    pub accumulated_damage: u8,
    /// Horror accumulated on this asset (for horror-soak / ally
    /// sanity). Distinct from the controlling investigator's horror.
    pub accumulated_horror: u8,
}

impl CardInPlay {
    /// Construct a fresh in-play instance: ready, no uses, no
    /// accumulated damage or horror. Caller threads the `instance_id`
    /// from the per-state counter.
    #[must_use]
    pub fn enter_play(code: CardCode, instance_id: CardInstanceId) -> Self {
        Self {
            code,
            instance_id,
            exhausted: false,
            uses: BTreeMap::new(),
            accumulated_damage: 0,
            accumulated_horror: 0,
        }
    }
}
