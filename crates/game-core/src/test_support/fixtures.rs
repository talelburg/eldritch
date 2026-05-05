//! Stock fixture constructors for tests.
//!
//! Tests constantly need a "reasonable" investigator or location to
//! place into a state. These helpers produce one with default-y values;
//! callers tweak fields after construction when something specific is
//! needed.

use crate::state::{Investigator, InvestigatorId, Location, LocationId, Skills};

/// A stock investigator with reasonable defaults.
///
/// - 3/3/3/3 skills, 8 health, 8 sanity, no damage or horror.
/// - 5 starting resources, 0 clues.
/// - 3 actions remaining.
/// - Not placed at any location (`current_location: None`).
///
/// Mutate fields directly after construction to customize.
#[must_use]
pub fn test_investigator(id: u32) -> Investigator {
    Investigator {
        id: InvestigatorId(id),
        name: format!("Test Investigator {id}"),
        current_location: None,
        skills: Skills {
            willpower: 3,
            intellect: 3,
            combat: 3,
            agility: 3,
        },
        max_health: 8,
        damage: 0,
        max_sanity: 8,
        horror: 0,
        clues: 0,
        resources: 5,
        actions_remaining: 3,
    }
}

/// A stock location with reasonable defaults.
///
/// - Shroud 2, 0 clues, revealed.
/// - No connections (caller adds them).
#[must_use]
pub fn test_location(id: u32, name: impl Into<String>) -> Location {
    Location {
        id: LocationId(id),
        name: name.into(),
        shroud: 2,
        clues: 0,
        revealed: true,
        connections: Vec::new(),
    }
}
