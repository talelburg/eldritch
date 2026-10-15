//! Card identifiers and per-instance in-play state.

use std::collections::BTreeMap;
use std::fmt;

use card_dsl::dsl::{UsageLimit, UsagePeriod};
use serde::{Deserialize, Serialize};

use crate::state::{GameState, InvestigatorId, Phase};

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

impl fmt::Display for CardCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// One of the decks a card effect can look into: an investigator's deck or
/// the encounter deck. Both keep their top card at index 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deck {
    /// This investigator's deck.
    Investigator(InvestigatorId),
    /// The encounter deck.
    Encounter,
}

/// A card-bearing zone, used as the `from` field on movement events
/// (e.g. [`Event::CardDiscarded`](crate::event::Event::CardDiscarded)).
///
/// Discard is a destination but never a `from` in the current event set — a
/// discard event names its destination as a [`DiscardPile`]. Further zones
/// land when they're needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Zone {
    /// A player's hand.
    Hand,
    /// A player's deck (top or bottom — events that need the
    /// distinction can record it separately).
    Deck,
    /// The encounter deck. Used as the `from` zone when a card taken off the
    /// encounter deck is discarded without being drawn (Rise of the Ghouls
    /// 01106's *"Discard cards from the top of the encounter deck"*).
    EncounterDeck,
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
    /// A card **put into play at** a location rather than into an
    /// investigator's area — act 01109b's *"Put the set-aside Lita Chantler
    /// into play in the Parlor"*. Not an attachment: see
    /// [`Event::CardPutIntoPlayAtLocation`](crate::event::Event::CardPutIntoPlayAtLocation).
    /// A card that leaves this zone for a player's control (Lita after a
    /// Parley) is in that player's play area from then on, so it leaves play
    /// from [`InPlay`](Self::InPlay).
    AtLocation,
    /// A card attached to an enemy.
    EnemyAttachment,
    /// An enemy itself, leaving play.
    Enemy,
    /// A location itself, leaving play — the Study 01111, which Trapped 01108
    /// removes from the game.
    Location,
    /// Out of the game entirely.
    /// `glossary/Removed_from_Game.md`: *"A card that has been removed from the
    /// game is placed away from the game area and has no further interaction
    /// with the game in any manner for the duration of its removal."* No event
    /// names it yet: [`Event::CardRemovedFromGame`](crate::event::Event::CardRemovedFromGame)
    /// names only the zone a card was removed *from*, and nothing in the corpus
    /// returns a removed card to the game.
    RemovedFromGame,
}

/// The discard pile a discarded card landed in — the `to` field of
/// [`Event::CardDiscarded`](crate::event::Event::CardDiscarded).
///
/// `glossary/Discard_Piles.md`: *"Any time a card is discarded, it is placed
/// faceup on top of its owner's discard pile. Encounter cards are owned by the
/// encounter deck."* So a pile is either an investigator's or the encounter
/// deck's; a scenario-owned card has no pile and is removed from the game
/// instead ([`Event::CardRemovedFromGame`](crate::event::Event::CardRemovedFromGame)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiscardPile {
    /// That investigator's discard pile.
    Investigator(InvestigatorId),
    /// The encounter discard pile.
    Encounter,
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
    /// and location clue pools. Most cards never carry clues (0). Required
    /// on the wire (#453).
    pub clues: u8,
    /// Doom sitting on this card instance (Arcane Initiate 01063: *"Place 1
    /// doom on it."*). It counts toward the agenda's threshold —
    /// `glossary/Doom.md`: *"Doom on cards other than the agenda (such as
    /// enemies, allies, locations, etc.) counts towards the amount of doom in
    /// play."* — through [`board::doom_in_play`](crate::engine::board::doom_in_play),
    /// and it leaves play with the card. Required on the wire (#453).
    pub doom: u8,
    /// Per-ability usage counter for "Limit X per \[period\]" caps. Key
    /// is the ability index within the card's `abilities()`; value
    /// records the [`UsageClock`] reading (round, phase and turn) the ability
    /// last fired at and how many times.
    ///
    /// Reset is **lazy** — when a query needs the count for the current
    /// period, it reads the record; if the stored clock is outside the
    /// period containing the current [`UsageClock`] (a different round for a
    /// round limit; a different round or phase for a phase limit; a different
    /// round, phase or active investigator for a turn limit), the count is
    /// treated as 0 (and overwritten on next fire). No period-end hook is
    /// required.
    ///
    /// Empty for cards with no [`UsageLimit`] on any ability. Required on the
    /// wire (#453).
    ///
    /// [`UsageLimit`]: card_dsl::dsl::UsageLimit
    pub ability_usage: BTreeMap<u8, AbilityUsageRecord>,
    /// **Who owns this card**, as distinct from who controls it — one of the
    /// three owners `GLOSSARY.md`'s **Owner / Controller** entry names.
    ///
    /// `glossary/Ownership_and_Control.md`: *"A card's owner is the player
    /// whose deck (or game area) held the card at the start of the game."*, and
    /// separately *"The scenario controls the cards in its out-of-play game
    /// areas"*. Control is **where the instance sits** — which collection holds
    /// it — and it moves when a card changes hands. Ownership does not:
    /// *"Cards by default enter play under their owner's control. Some
    /// abilities may cause cards to change control during a game."*
    ///
    /// **Where a card goes when it leaves play is a question about its owner.**
    /// That is what makes Lita Chantler 01117's removal *derive* rather than be
    /// special-cased — her ruling states the derivation outright
    /// (<https://arkhamdb.com/card/01117>): *"If Lita leaves play while a
    /// player controls her temporarily during 'The Gathering' scenario **(i.e.
    /// while she is technically not a part of that player's deck)**, remove her
    /// from the game (do not place her into any discard pile)."* The
    /// parenthetical is the premise; encoding only the conclusion would throw it
    /// away.
    ///
    /// Fixed when the card enters play: [`enter_play`](Self::enter_play) takes
    /// it as a required argument, because the caller knows where the card came
    /// from and the card's metadata does not. Required on the wire (#453).
    pub owner: Owner,
}

/// **Who owns a card** — one of three owners (`GLOSSARY.md`, **Owner /
/// Controller**). Recorded on every [`CardInPlay`] and on every
/// [`Enemy`](crate::state::Enemy), and fixed when it enters play.
///
/// "Owned by no player" is deliberately two cases, because the two leave play
/// differently: an encounter card goes to the encounter discard, and a
/// scenario-owned card is removed from the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Owner {
    /// The investigator whose deck held the card at the start of the game —
    /// including a weakness with an encounter cardtype, whose owner is its
    /// bearer (`glossary/Weakness.md`: *"The bearer of a weakness is the
    /// investigator who started the game with the weakness in his or her deck
    /// or play area."*).
    Investigator(InvestigatorId),
    /// The encounter deck, which owns encounter cards
    /// (`glossary/Discard_Piles.md`: *"Encounter cards are owned by the
    /// encounter deck."*).
    EncounterDeck,
    /// The scenario: a card that started set aside or in the scenario's play
    /// area and has no out-of-play pile to return to — Lita Chantler 01117 in
    /// The Gathering.
    Scenario,
}

/// A reading of the clock a [`UsagePeriod`] is measured on: the
/// [`GameState::round`](crate::state::GameState::round), the
/// [`GameState::phase`](crate::state::GameState::phase), and whose turn it is
/// ([`GameState::active_investigator`](crate::state::GameState::active_investigator)).
///
/// The turn is the active investigator rather than a turn counter: each
/// investigator takes one turn per Investigation phase, so round, phase and
/// investigator together name the turn. Outside the Investigation phase the
/// engine clears the active investigator, so `turn` is `None` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UsageClock {
    pub round: u32,
    pub phase: Phase,
    pub turn: Option<InvestigatorId>,
}

impl UsageClock {
    /// A clock reading. `#[non_exhaustive]`, so construct it through here.
    #[must_use]
    pub fn new(round: u32, phase: Phase, turn: Option<InvestigatorId>) -> Self {
        Self { round, phase, turn }
    }

    /// The clock as `state` reads now.
    #[must_use]
    pub fn of(state: &GameState) -> Self {
        Self::new(state.round, state.phase, state.active_investigator)
    }

    /// Whether `self` and `now` fall in the same `period`. A `Round` period
    /// ignores the phase, so a round limit does not reset at a phase change; a
    /// `Phase` period needs both to match, since the same phase recurs every
    /// round; a `Turn` period also needs the same investigator's turn.
    fn same_period(self, period: UsagePeriod, now: Self) -> bool {
        match period {
            UsagePeriod::Round => self.round == now.round,
            UsagePeriod::Phase => self.round == now.round && self.phase == now.phase,
            UsagePeriod::Turn => self == now,
        }
    }
}

/// One ability's firing record for "Limit X per \[period\]" tracking.
///
/// `clock` is the [`UsageClock`] reading at last fire — the clock every
/// [`UsagePeriod`] reads its part of. `count` is the number of fires during
/// the period that clock reading falls in (compared against the ability's
/// [`UsageLimit::count`](card_dsl::dsl::UsageLimit::count) to gate further
/// fires).
///
/// The record stores the whole clock whatever the ability's period, so its
/// shape does not vary by limit; the period is applied when the record is read
/// or bumped.
///
/// `#[non_exhaustive]` so future periods (`Game`) can add fields without
/// breaking downstream construction. Use [`AbilityUsageRecord::new`] to
/// construct from outside the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AbilityUsageRecord {
    pub clock: UsageClock,
    pub count: u8,
}

impl AbilityUsageRecord {
    /// Construct a usage record. The fields are `pub`, but the struct
    /// is `#[non_exhaustive]` so downstream code (tests, future
    /// scenario modules) must go through this constructor.
    #[must_use]
    pub fn new(clock: UsageClock, count: u8) -> Self {
        Self { clock, count }
    }
}

/// Whether the ability at `ability_index` has reached its
/// [`UsageLimit::count`] for the period containing the clock reading `now`,
/// reading the firing record out of `usage`. Shared by [`CardInPlay`] and
/// [`Investigator`](crate::state::Investigator) so both usage-bearing
/// sources apply the same lazy-reset semantics (see the field docs on
/// [`CardInPlay::ability_usage`]). `None` limit ⇒ no cap ⇒ `false`.
#[must_use]
pub fn usage_exhausted(
    usage: &BTreeMap<u8, AbilityUsageRecord>,
    ability_index: u8,
    limit: Option<UsageLimit>,
    now: UsageClock,
) -> bool {
    let Some(limit) = limit else {
        return false;
    };
    usage.get(&ability_index).is_some_and(|record| {
        record.clock.same_period(limit.period, now) && record.count >= limit.count
    })
}

/// Record one firing of the ability at `ability_index` against the
/// `period` containing the clock reading `now`, into `usage` (lazy reset when
/// the stored record is for a stale period). Shared by [`CardInPlay`] and
/// [`Investigator`](crate::state::Investigator).
pub fn bump_usage(
    usage: &mut BTreeMap<u8, AbilityUsageRecord>,
    ability_index: u8,
    period: UsagePeriod,
    now: UsageClock,
) {
    let record = usage
        .entry(ability_index)
        .or_insert(AbilityUsageRecord::new(now, 0));
    if !record.clock.same_period(period, now) {
        record.count = 0;
    }
    record.clock = now;
    record.count = record.count.saturating_add(1);
}

impl CardInPlay {
    /// Construct a fresh in-play instance owned by `owner`: ready, no uses, no
    /// accumulated damage or horror. Caller threads the `instance_id`
    /// from the per-state counter.
    ///
    /// `owner` has no default: the caller states where the card came from —
    /// drawn from an investigator's deck or played by them → that
    /// investigator, drawn from the encounter deck → the encounter deck, set
    /// aside → whichever of the two owns it (see [`Owner`]).
    #[must_use]
    pub fn enter_play(code: CardCode, instance_id: CardInstanceId, owner: Owner) -> Self {
        Self {
            code,
            instance_id,
            exhausted: false,
            uses: BTreeMap::new(),
            accumulated_damage: 0,
            accumulated_horror: 0,
            clues: 0,
            doom: 0,
            ability_usage: BTreeMap::new(),
            owner,
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
        now: UsageClock,
    ) -> bool {
        usage_exhausted(&self.ability_usage, ability_index, limit, now)
    }

    /// Record one firing of the ability at `ability_index` against the
    /// current `period`. Resets the counter if the stored record is for
    /// a stale period (lazy reset — see the field-level docs on
    /// [`ability_usage`](Self::ability_usage)).
    pub fn bump_ability_usage(&mut self, ability_index: u8, period: UsagePeriod, now: UsageClock) {
        bump_usage(&mut self.ability_usage, ability_index, period, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every owner survives the wire.
    #[test]
    fn each_owner_round_trips_through_the_wire() {
        for owner in [
            Owner::Investigator(InvestigatorId(2)),
            Owner::EncounterDeck,
            Owner::Scenario,
        ] {
            let c = CardInPlay::enter_play(CardCode("_x".into()), CardInstanceId(1), owner);
            let json = serde_json::to_value(&c).expect("serialize");
            assert_eq!(
                serde_json::from_value::<CardInPlay>(json).expect("deserialize"),
                c,
            );
        }
    }

    #[test]
    fn enter_play_defaults_clues_to_zero() {
        let c = CardInPlay::enter_play(CardCode("_x".into()), CardInstanceId(1), Owner::Scenario);
        assert_eq!(c.clues, 0);
    }

    #[test]
    fn omitting_any_required_field_is_rejected() {
        // `clues`, `doom`, `ability_usage` and `owner` are required on the
        // wire (#453): a payload missing one fails loudly rather than
        // silently defaulting.
        let c = CardInPlay::enter_play(CardCode("_x".into()), CardInstanceId(1), Owner::Scenario);
        let full = serde_json::to_value(&c).expect("serialize");
        serde_json::from_value::<CardInPlay>(full.clone()).expect("full object deserializes");
        for field in ["clues", "doom", "ability_usage", "owner"] {
            let mut v = full.clone();
            v.as_object_mut()
                .expect("card serializes to a JSON object")
                .remove(field)
                .unwrap_or_else(|| panic!("`{field}` should be present in the serialized form"));
            assert!(
                serde_json::from_value::<CardInPlay>(v).is_err(),
                "omitting `{field}` must be rejected, not defaulted"
            );
        }
    }
}
