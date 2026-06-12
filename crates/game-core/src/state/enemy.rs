//! Enemies: hostile creatures that engage investigators, attack, and
//! are defeated through combat.

use serde::{Deserialize, Serialize};

use super::{card::CardCode, investigator::InvestigatorId, location::LocationId};
use crate::card_data::Prey;

/// Stable identifier for an enemy within a scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnemyId(pub u32);

/// An enemy in play during a scenario.
///
/// Minimal shape needed for Fight / Evade actions in this PR. Fields
/// that don't influence those actions are deferred to the issues that
/// will exercise them:
/// - `aloof`: spawn-time engagement rule (separate issue).
///
/// Adding each field with its real shape when its consumer lands keeps
/// us from baking placeholder semantics that turn out to be wrong.
///
/// # Invariants
///
/// - `damage <= max_health`. Reaching equality means the enemy is
///   defeated and should be removed from `GameState::enemies`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Enemy {
    /// Stable identifier within this scenario.
    pub id: EnemyId,
    /// Display name.
    pub name: String,
    /// Printed `ArkhamDB` code (e.g. `"01116"` for the Ghoul Priest).
    /// Carried so framework effects keyed on a specific enemy — Act 3's
    /// "If the Ghoul Priest is Defeated, advance." — can match after the
    /// enemy leaves `state.enemies`.
    pub code: CardCode,
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
    /// Whether this enemy has the Hunter keyword (Rules Reference p.12):
    /// a ready, unengaged hunter moves toward the nearest investigator
    /// during Enemy-phase step 3.2.
    pub hunter: bool,
    /// Prey instruction (Rules Reference p.17): which investigator the
    /// enemy pursues / engages when it has a choice. `Prey::Default`
    /// for enemies with no printed prey line.
    pub prey: Prey,
}

#[cfg(test)]
mod hunter_prey_field_tests {
    use super::*;

    #[test]
    fn enemy_carries_hunter_and_prey() {
        let e = Enemy {
            id: EnemyId(1),
            name: "Ghoul Priest".into(),
            fight: 4,
            evade: 4,
            max_health: 5,
            damage: 0,
            attack_damage: 2,
            attack_horror: 2,
            current_location: None,
            exhausted: false,
            traits: vec!["Humanoid".into(), "Monster".into(), "Elite".into()],
            engaged_with: None,
            hunter: true,
            prey: Prey::Default,
            code: crate::CardCode::new("01116"),
        };
        assert!(e.hunter);
        assert_eq!(e.prey, Prey::Default);
    }

    #[test]
    fn test_enemy_fixture_carries_a_code() {
        let e = crate::test_support::test_enemy(7, "Ghoul");
        assert!(!e.code.as_str().is_empty(), "every enemy carries its printed code");
    }
}
