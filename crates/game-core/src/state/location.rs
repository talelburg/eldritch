//! Locations: places investigators move between.

use serde::{Deserialize, Serialize};

use card_dsl::card_data::ClueValue;

use super::card::{CardCode, CardInPlay};

crate::state::define_id! {
    /// Stable identifier for a location within a scenario.
    pub struct LocationId;
}

/// A location in the current scenario.
///
/// Phase-1 minimal shape; later phases will add e.g. encounter-set
/// affiliation, victory points, location-specific effects, and
/// hidden-information state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Location {
    /// Stable identifier within this scenario.
    pub id: LocationId,
    /// Printed `ArkhamDB` location code (e.g. `"01111"` for Study).
    /// Stable across instances of the same printed location — two
    /// copies of the same card in play would carry the same `code`
    /// but distinct `id`s.
    ///
    /// Used by encounter-enemy spawn rules to address a specific
    /// location by its printed identifier (see
    /// [`card_dsl::card_data::SpawnLocation::Specific`]).
    pub code: CardCode,
    /// Display name.
    pub name: String,
    /// Difficulty modifier added to investigate tests at this location.
    pub shroud: u8,
    /// Clues currently on the location.
    pub clues: u8,
    /// The printed clue value on the location card (used to place clues
    /// at reveal time). `PerInvestigator(n)` means `n × investigator_count`
    /// clues are placed; `Fixed(n)` places exactly `n` regardless of count.
    pub printed_clues: ClueValue,
    /// Whether the location is face-up. Unrevealed locations show only
    /// their "back" name and aren't yet investigatable.
    pub revealed: bool,
    /// Locations physically connected to this one (movement targets).
    pub connections: Vec<LocationId>,
    /// Encounter cards attached to this location (e.g. Obscuring Fog
    /// 01168 grants `+2` shroud while attached). Empty for the common
    /// case; discarded back to the encounter discard via
    /// [`Effect::DiscardSelf`](crate::dsl::Effect::DiscardSelf).
    pub attachments: Vec<CardInPlay>,
    /// Cards **put into play at** this location, controlled by no
    /// investigator — Lita Chantler 01117, whom act 01109's reverse puts
    /// *"into play in the Parlor"* before anyone takes control of her.
    ///
    /// A distinct zone from [`attachments`](Self::attachments), not a reuse of
    /// it: `glossary/Attach_To.md` gives an attachment a lifetime bound to its
    /// host — *"an attachment remains attached until either the attachment or
    /// the game element to which it is attached leaves play (in which case the
    /// attachment is discarded)"* — and an audience
    /// ([`ModifierAudience::AttachedCard`](crate::dsl::ModifierAudience::AttachedCard))
    /// that reads the host's stats. A card put into play *in* a location has
    /// neither. The rules name this zone directly:
    /// `glossary/In_Play_and_Out_of_Play.md` — *"each encounter card in a
    /// investigator's threat area **or at a location**, are all considered in
    /// play."*
    ///
    /// Deliberately **not** reachable from any investigator's
    /// [`controlled_card_instances`](crate::state::Investigator::controlled_card_instances):
    /// an uncontrolled asset is not an eligible soak target, since
    /// `glossary/Asset_Cards.md` restricts assignment to assets *"he or she
    /// controls"*. Empty for the common case.
    ///
    /// # Leaving the zone
    ///
    /// Two exits, and neither is implemented yet — the zone has exactly one
    /// occupant today, and she leaves it only through #772's take-control
    /// Parley.
    ///
    /// **Take control** moves the *same* [`CardInPlay`] out of here and into
    /// the new controller's `cards_in_play`. Moving the instance rather than
    /// minting a fresh one is the contract: the struct carries
    /// `accumulated_damage`, `accumulated_horror`, `uses` and `ability_usage`,
    /// and a card that changes hands is the same card — *"When you 'take
    /// control' of a card, it enters your play area (not your hand)"*
    /// (<https://arkhamdb.com/card/01117>). The play-card path's
    /// mint-on-entry is therefore the wrong shape to reuse.
    ///
    /// **Leaving play** goes to the same destination the card's own type
    /// dictates, except where the scenario overrides it — for Lita, held
    /// temporarily and owned by nobody's deck, the override is explicit: *"If
    /// Lita leaves play while a player controls her temporarily during 'The
    /// Gathering' scenario (i.e. while she is technically not a part of that
    /// player's deck), remove her from the game (do not place her into any
    /// discard pile)"* (<https://arkhamdb.com/card/01117>).
    pub cards_at_location: Vec<CardInPlay>,
}

impl Location {
    /// Construct a revealed location with no connections, from its
    /// printed identity and stats (`code`, `name`, `shroud`, `clues`).
    ///
    /// Set `connections` (and `revealed`, for cards that enter play
    /// face-down) afterward via the public fields — those are
    /// scenario-layout concerns, not printed on the card. This is the
    /// cross-crate constructor scenarios use to build their board; the
    /// struct is `#[non_exhaustive]`, so a struct literal won't compile
    /// outside `game-core`.
    #[must_use]
    pub fn new(
        id: LocationId,
        code: CardCode,
        name: impl Into<String>,
        shroud: u8,
        clues: u8,
    ) -> Self {
        Self {
            id,
            code,
            name: name.into(),
            shroud,
            clues,
            printed_clues: ClueValue::Fixed(clues),
            revealed: true,
            connections: Vec::new(),
            attachments: Vec::new(),
            cards_at_location: Vec::new(),
        }
    }
}

#[cfg(test)]
mod location_code_tests {
    use super::*;
    use crate::state::{CardCode, CardInstanceId};

    #[test]
    fn location_carries_code_field() {
        let loc = Location {
            id: LocationId(1),
            code: CardCode("01112".into()),
            name: "Hallway".into(),
            shroud: 2,
            clues: 0,
            printed_clues: ClueValue::Fixed(0),
            revealed: true,
            connections: Vec::new(),
            attachments: Vec::new(),
            cards_at_location: Vec::new(),
        };
        assert_eq!(loc.code, CardCode("01112".into()));
    }

    #[test]
    fn location_new_builds_revealed_unconnected_location() {
        let loc = Location::new(LocationId(3), CardCode("01111".into()), "Study", 2, 2);
        assert_eq!(loc.id, LocationId(3));
        assert_eq!(loc.code, CardCode("01111".into()));
        assert_eq!(loc.name, "Study");
        assert_eq!(loc.shroud, 2);
        assert_eq!(loc.clues, 2);
        assert!(loc.revealed, "new locations are revealed");
        assert!(loc.connections.is_empty(), "new locations are unconnected");
        assert!(
            loc.cards_at_location.is_empty(),
            "new locations hold no cards put into play at them",
        );
    }

    #[test]
    fn location_serde_roundtrip_preserves_cards_at_location() {
        // The uncontrolled-asset zone is board state a client renders, so it
        // has to survive the wire — and, like `attachments`, it carries no
        // `#[serde(default)]`, so a payload that omits it fails loudly (#453)
        // rather than silently emptying the Parlor.
        let mut original = Location::new(LocationId(4), CardCode("01115".into()), "Parlor", 2, 0);
        original.cards_at_location.push(CardInPlay::enter_play(
            CardCode::new("01117"),
            CardInstanceId(7),
        ));
        let json = serde_json::to_value(&original).expect("serialize");
        let back: Location = serde_json::from_value(json.clone()).expect("deserialize");
        assert_eq!(back.cards_at_location, original.cards_at_location);

        let mut without = json;
        without
            .as_object_mut()
            .expect("a location serializes to a JSON object")
            .remove("cards_at_location")
            .expect("`cards_at_location` should be present in the serialized form");
        assert!(
            serde_json::from_value::<Location>(without).is_err(),
            "omitting `cards_at_location` must be rejected, not defaulted",
        );
    }

    #[test]
    fn location_serde_roundtrip_preserves_code() {
        let original = Location {
            id: LocationId(2),
            code: CardCode("_synth_loc".into()),
            name: "Demo Location".into(),
            shroud: 1,
            clues: 3,
            printed_clues: ClueValue::Fixed(3),
            revealed: false,
            connections: vec![LocationId(1)],
            attachments: Vec::new(),
            cards_at_location: Vec::new(),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Location = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, original.id);
        assert_eq!(back.code, original.code);
        assert_eq!(back.name, original.name);
    }
}
