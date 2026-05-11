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
///
/// # Invariants
///
/// - `damage` may exceed `max_health` transiently — when that happens
///   the apply loop's damage helpers flip `status` to [`Status::Killed`]
///   and emit [`Event::InvestigatorDefeated`]. Symmetric for `horror`
///   / `max_sanity` / [`Status::Insane`]. The numeric fields are
///   `u8` so they don't wrap; the threshold check is what defines
///   defeat.
/// - Once `status != Status::Active`, the investigator is "out of
///   play": damage / horror helpers no-op, the engine doesn't let
///   them take actions, and card effects targeting investigators
///   should filter by status.
///
/// [`Event::InvestigatorDefeated`]: crate::event::Event::InvestigatorDefeated
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Investigator {
    /// Stable identifier within this scenario.
    pub id: InvestigatorId,
    /// Display name.
    pub name: String,
    /// Location the investigator is currently at, or `None` if they are
    /// "between locations" (resigned, defeated, or in scenario setup
    /// before initial placement).
    pub current_location: Option<LocationId>,
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
    /// Active / Killed / Insane / Resigned. See [`Status`].
    pub status: Status,
}

/// Whether an investigator is still active in the scenario, and if not,
/// how they left play.
///
/// Resigned is a placeholder slot until the Resign action lands; the
/// engine doesn't currently produce that variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Status {
    /// Investigator is in play and can take actions.
    #[default]
    Active,
    /// Investigator was killed (`damage >= max_health`).
    Killed,
    /// Investigator was driven insane (`horror >= max_sanity`).
    Insane,
    /// Investigator chose to resign from the scenario. Not yet
    /// produced by the engine; the Resign action is downstream.
    Resigned,
}

/// Why an investigator was defeated. Carried on
/// [`Event::InvestigatorDefeated`] so consumers (campaign log,
/// after-defeat triggers) know the cause without re-reading state.
///
/// [`Event::InvestigatorDefeated`]: crate::event::Event::InvestigatorDefeated
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DefeatCause {
    /// Damage reached `max_health`.
    Damage,
    /// Horror reached `max_sanity`.
    Horror,
    /// Investigator resigned. Not yet produced; reserved for the
    /// Resign action.
    Resigned,
}

/// The four base skill values.
///
/// Deliberately NOT `#[non_exhaustive]`: the four skills are fixed by
/// FFG's rules. Card effects modify these values at query time; they
/// don't add new fields.
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
