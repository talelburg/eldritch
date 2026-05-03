//! Locations: places investigators move between.

use serde::{Deserialize, Serialize};

/// Stable identifier for a location within a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LocationId(pub u32);

/// A location in the current scenario.
///
/// Phase-1 minimal shape; later phases will add e.g. encounter-set
/// affiliation, victory points, location-specific effects, and
/// hidden-information state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Location {
    /// Stable identifier within this scenario.
    pub id: LocationId,
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
