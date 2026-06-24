//! Card identifiers and per-instance in-play state.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dsl::{UsageLimit, UsagePeriod};

/// `ArkhamDB` card code (e.g. `"01030"` for Magnifying Glass).
///
/// Newtype over `String` so we can't accidentally pass arbitrary strings
/// where a card code is expected. Serializes transparently as a string
/// so the wire format matches the upstream JSON.
///
/// The cards crate's lookups (`cards::by_code`, `cards::abilities_for`)
/// take `&str`; deref or call `.as_str()` to bridge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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
    /// An investigator's threat area — the play area holding encounter
    /// cards engaged with / affecting them (Rules Reference p.20).
    /// Cards there are at the investigator's location. Used as the
    /// `from` zone when a threat-area card is discarded.
    ThreatArea,
    /// A location's attachment zone — encounter cards attached to a
    /// location (Obscuring Fog 01168). Used as the `from` zone when an
    /// attachment is discarded.
    LocationAttachment,
}

crate::state::define_id! {
    /// Unique identifier for a specific copy of a card in play.
    ///
    /// Minted by the engine when a card enters play (via the per-state
    /// `card_instance_ids` [`Counter`](crate::state::Counter)). Lets card
    /// effects target a specific copy when an investigator has multiple of
    /// the same card in play (e.g. two copies of Magnifying Glass —
    /// exhausting one shouldn't exhaust the other).
    pub struct CardInstanceId;
}

// `UseKind` is defined in `card-dsl` (the lowest layer) so the printed
// metadata (`card_data::Uses`) and this runtime pool share one type;
// re-exported here at its historical `game_core::state` path.
pub use card_dsl::card_data::UseKind;

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
    /// Clues sitting on this card instance (Cover Up 01007 enters the
    /// threat area "with 3 clues on it"). Distinct from the investigator
    /// and location clue pools; defaults to 0. Most cards never carry
    /// clues, so absent on the wire → 0.
    #[serde(default)]
    pub clues: u8,
    /// Per-ability usage counter for "Limit X per \[period\]" caps. Key
    /// is the ability index within the card's `abilities()`; value
    /// records the last round the ability fired and how many times.
    ///
    /// Reset is **lazy** — when a query needs the count for the current
    /// round, it reads the record; if the stored `round` doesn't match
    /// [`GameState::round`](crate::state::GameState::round), the count
    /// is treated as 0 (and overwritten on next fire). No explicit
    /// round-end hook is required, which matters because Phase 3 has
    /// no round-cycling framework yet — rounds tick by a test or future
    /// scenario action mutating `state.round`.
    ///
    /// Empty for cards with no [`UsageLimit`] on any ability.
    ///
    /// [`UsageLimit`]: crate::dsl::UsageLimit
    #[serde(default)]
    pub ability_usage: BTreeMap<u8, AbilityUsageRecord>,
}

/// One ability's firing record for "Limit X per \[period\]" tracking.
///
/// `round` is the value of
/// [`GameState::round`](crate::state::GameState::round) at last fire.
/// `count` is the number of fires during that round (compared against
/// the ability's [`UsageLimit::count`](crate::dsl::UsageLimit::count)
/// to gate further fires).
///
/// `#[non_exhaustive]` so future periods (`Phase`, `Game`) can add
/// fields without breaking downstream construction. Use
/// [`AbilityUsageRecord::new`] to construct from outside the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AbilityUsageRecord {
    pub round: u32,
    pub count: u8,
}

impl AbilityUsageRecord {
    /// Construct a usage record. The fields are `pub`, but the struct
    /// is `#[non_exhaustive]` so downstream code (tests, future
    /// scenario modules) must go through this constructor.
    #[must_use]
    pub fn new(round: u32, count: u8) -> Self {
        Self { round, count }
    }
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
            clues: 0,
            ability_usage: BTreeMap::new(),
        }
    }

    /// Returns `true` if the ability at `ability_index` has already
    /// reached its [`UsageLimit::count`] for the current period.
    ///
    /// `None` for `limit` (the ability has no printed "Limit X per …"
    /// clause) always returns `false` — that ability has no per-period
    /// cap; the rules' default once-per-occurrence cap on reaction
    /// abilities is enforced by the reaction-window dispatch itself,
    /// not by this counter.
    #[must_use]
    pub fn is_usage_exhausted(
        &self,
        ability_index: u8,
        limit: Option<UsageLimit>,
        current_round: u32,
    ) -> bool {
        let Some(limit) = limit else {
            return false;
        };
        match limit.period {
            UsagePeriod::Round => {
                let Some(record) = self.ability_usage.get(&ability_index) else {
                    return false;
                };
                if record.round != current_round {
                    return false;
                }
                record.count >= limit.count
            }
        }
    }

    /// Record one firing of the ability at `ability_index` against the
    /// current period. Resets the counter if the stored record is for
    /// a stale period (lazy reset — see the field-level docs on
    /// [`ability_usage`](Self::ability_usage)).
    pub fn bump_ability_usage(&mut self, ability_index: u8, current_round: u32) {
        let record = self
            .ability_usage
            .entry(ability_index)
            .or_insert(AbilityUsageRecord {
                round: current_round,
                count: 0,
            });
        if record.round != current_round {
            record.round = current_round;
            record.count = 0;
        }
        record.count = record.count.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{CardCode, CardInPlay, CardInstanceId};

    #[test]
    fn enter_play_defaults_clues_to_zero() {
        let c = CardInPlay::enter_play(CardCode("_x".into()), CardInstanceId(1));
        assert_eq!(c.clues, 0);
    }

    #[test]
    fn card_in_play_deserializes_when_clues_field_absent() {
        // A state serialized before `clues` existed must still load (field
        // defaults to 0), mirroring the `ability_usage` serde-default test.
        let json = r#"{
            "code": "_x", "instance_id": 1, "exhausted": false,
            "uses": {}, "accumulated_damage": 0, "accumulated_horror": 0,
            "ability_usage": {}
        }"#;
        let c: CardInPlay = serde_json::from_str(json).expect("deserialize");
        assert_eq!(c.clues, 0);
    }
}
