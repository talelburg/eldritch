//! Enemies: hostile creatures that engage investigators, attack, and
//! are defeated through combat.

use serde::{Deserialize, Serialize};

use super::{investigator::InvestigatorId, location::LocationId};

/// Stable identifier for an enemy within a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnemyId(pub u32);

/// An enemy in play during a scenario.
///
/// Minimal shape needed for Fight / Evade actions in this PR. Fields
/// that don't influence those actions are deferred to the issues that
/// will exercise them:
/// - `aloof`, `prey`: spawn-time engagement rule (separate issue).
/// - `hunter`, `prey`: hunter movement during the enemy phase (#71).
///
/// Adding each field with its real shape when its consumer lands keeps
/// us from baking placeholder semantics that turn out to be wrong.
///
/// # Invariants
///
/// - `damage <= max_health`. Reaching equality means the enemy is
///   defeated and should be removed from `GameState::enemies`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Enemy {
    /// Stable identifier within this scenario.
    pub id: EnemyId,
    /// Display name.
    pub name: String,
    /// Difficulty for Fight tests against this enemy.
    pub fight: i8,
    /// Difficulty for Evade tests against this enemy.
    pub evade: i8,
    /// Maximum health.
    pub max_health: u8,
    /// Current damage suffered. Invariant: `<= max_health`.
    pub damage: u8,
    /// Damage this enemy deals on an attack (`AoO` + enemy phase).
    pub attack_damage: u8,
    /// Horror this enemy deals on an attack.
    pub attack_horror: u8,
    /// Where the enemy is on the location map. `None` for enemies
    /// that haven't spawned (or have left play).
    pub current_location: Option<LocationId>,
    /// Whether the enemy is exhausted. Exhausted enemies don't make
    /// attacks of opportunity and don't activate during the enemy phase.
    pub exhausted: bool,
    /// Card-text traits (Monster, Humanoid, Cultist, Elite, etc.).
    /// "Elite" is a trait, not a separate field — it's how cards
    /// filter ("deal 1 damage to a non-Elite enemy", "automatically
    /// evade a non-Elite enemy at your location", etc.), and the same
    /// mechanism handles every other trait filter uniformly. Same
    /// encoding as in the upstream card JSON's `traits` field.
    pub traits: Vec<String>,
    /// Which investigator (if any) the enemy is currently engaged
    /// with. Engagement is stored on the enemy because the rulebook
    /// frames enemies as engaging investigators, not vice versa, and
    /// the lookup "which enemies is this investigator engaged with"
    /// is just a scan of `state.enemies`.
    pub engaged_with: Option<InvestigatorId>,
}
