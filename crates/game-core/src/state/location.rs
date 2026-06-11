//! Locations: places investigators move between.

use serde::{Deserialize, Serialize};

use super::card::CardCode;

/// Stable identifier for a location within a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LocationId(pub u32);

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
    /// Whether the location is face-up. Unrevealed locations show only
    /// their "back" name and aren't yet investigatable.
    pub revealed: bool,
    /// Locations physically connected to this one (movement targets).
    pub connections: Vec<LocationId>,
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
            revealed: true,
            connections: Vec::new(),
        }
    }
}

#[cfg(test)]
mod location_code_tests {
    use super::*;
    use crate::state::CardCode;

    #[test]
    fn location_carries_code_field() {
        let loc = Location {
            id: LocationId(1),
            code: CardCode("01112".into()),
            name: "Hallway".into(),
            shroud: 2,
            clues: 0,
            revealed: true,
            connections: Vec::new(),
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
    }

    #[test]
    fn location_serde_roundtrip_preserves_code() {
        let original = Location {
            id: LocationId(2),
            code: CardCode("_synth_loc".into()),
            name: "Demo Location".into(),
            shroud: 1,
            clues: 3,
            revealed: false,
            connections: vec![LocationId(1)],
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Location = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, original.id);
        assert_eq!(back.code, original.code);
        assert_eq!(back.name, original.name);
    }
}
