//! Investigators: the players' avatars in the game.

use serde::{Deserialize, Serialize};

use super::location::LocationId;

/// Stable identifier for an investigator within a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InvestigatorId(pub u32);

/// An investigator's full state during a scenario.
///
/// Phase-1 minimal shape; fields will grow as later phases need them
/// (mental/physical trauma carried in from the campaign log, traits,
/// passive ability flags, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Investigator {
    /// Stable identifier within this scenario.
    pub id: InvestigatorId,
    /// Display name.
    pub name: String,
    /// Location the investigator is currently at.
    pub current_location: LocationId,
    /// Skill values.
    pub skills: Skills,
    /// Maximum health (physical hit points).
    pub max_health: u8,
    /// Current physical damage suffered.
    pub damage: u8,
    /// Maximum sanity.
    pub max_sanity: u8,
    /// Current horror suffered.
    pub horror: u8,
    /// Clues currently held by the investigator.
    pub clues: u8,
    /// Resources currently held.
    pub resources: u8,
    /// Action points remaining this turn (refreshed at the start of each
    /// investigation phase).
    pub actions_remaining: u8,
}

/// The four base skill values.
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
