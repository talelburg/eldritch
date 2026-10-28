//! Enemies: hostile creatures that engage investigators, attack, and
//! are defeated through combat.

use card_dsl::card_data::{CardKind, CardMetadata, HealthValue, Prey};
use serde::{Deserialize, Serialize};

use crate::state::{CardCode, CardInPlay, InvestigatorId, LocationId, Owner};

crate::state::define_id! {
    /// Stable identifier for an enemy within a scenario.
    pub struct EnemyId;
}

/// An enemy in play during a scenario.
///
/// A field is added with its real shape when a consumer needs it, which
/// keeps placeholder semantics out of the state. TODO(#579): the Aloof
/// keyword (the spawn-time engagement rule) has no field.
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
    /// Which investigator (if any) the enemy is currently engaged
    /// with. Engagement is stored on the enemy because the rulebook
    /// frames enemies as engaging investigators, not vice versa, and
    /// the lookup "which enemies is this investigator engaged with"
    /// is just a scan of `state.enemies`.
    ///
    /// Every production write goes through the engagement exits
    /// (`engine::dispatch::engagement`), which log the paired
    /// `EnemyEngaged` / `EnemyDisengaged` events; test fixtures may still set
    /// it directly. TODO(#611): make the field private once reads funnel
    /// through the module too.
    pub engaged_with: Option<InvestigatorId>,
    /// Whether this enemy has the Hunter keyword (Rules Reference p.12):
    /// a ready, unengaged hunter moves toward the nearest investigator
    /// during Enemy-phase step 3.2.
    pub hunter: bool,
    /// Prey instruction (Rules Reference p.17): which investigator the
    /// enemy pursues / engages when it has a choice. `Prey::Default`
    /// for enemies with no printed prey line.
    pub prey: Prey,
    /// Whether this enemy has the Retaliate keyword (Rules Reference
    /// p.18): after an investigator fails a Fight test against this
    /// enemy while it is ready, it performs an attack against that
    /// investigator (without exhausting). `false` for enemies with no
    /// printed Retaliate line.
    pub retaliate: bool,
    /// Printed Victory value (Rules Reference p.21). `Some(n)` places the
    /// enemy in the victory display when it is defeated; `None` for enemies
    /// that award no victory points.
    pub victory: Option<u8>,
    /// Cards attached to this enemy — the enemy-side counterpart of
    /// [`Location::attachments`](crate::state::Location::attachments).
    /// Treacheries that attach to an enemy modify it from here
    /// (Towering Beasts 02256: *"Attached enemy gets +1 fight and +1
    /// health."*), which is why the modified-value sweep reads it.
    ///
    /// No effect populates it yet — the DSL has
    /// [`AttachSelfToLocation`](card_dsl::dsl::Effect::AttachSelfToLocation)
    /// and no enemy counterpart — so it is empty on every board the
    /// engine builds today. Required on the wire.
    pub attachments: Vec<CardInPlay>,
    /// **Who owns this enemy** — the same three-valued [`Owner`] a card in
    /// play carries, fixed when the enemy enters play. An enemy drawn from the
    /// encounter deck is the encounter deck's; a weakness enemy is its
    /// bearer's (`glossary/Weakness.md`: *"The bearer of a weakness is the
    /// investigator who started the game with the weakness in his or her deck
    /// or play area."*). Required on the wire.
    pub owner: Owner,
}

impl Enemy {
    /// The enemy `metadata` prints, as it enters play: undamaged, ready,
    /// unengaged, with nothing attached. `None` when `metadata` is not an
    /// enemy card.
    ///
    /// A per-investigator health value scales by `investigator_count`, the
    /// investigators who started the scenario (Rules Reference p.12). A card
    /// that prints no health gets 1. A fight or evade above `i8::MAX`
    /// saturates.
    pub(crate) fn from_metadata(
        id: EnemyId,
        metadata: &CardMetadata,
        location: Option<LocationId>,
        owner: Owner,
        investigator_count: u8,
    ) -> Option<Self> {
        let CardKind::Enemy {
            health,
            fight,
            evade,
            damage,
            horror,
            hunter,
            retaliate,
            prey,
            victory,
            ..
        } = &metadata.kind
        else {
            return None;
        };
        let max_health = match health {
            Some(HealthValue::Fixed(n)) => *n,
            Some(HealthValue::PerInvestigator(n)) => n.saturating_mul(investigator_count),
            None => 1,
        };
        Some(Self {
            id,
            name: metadata.name.clone(),
            code: CardCode::new(metadata.code.clone()),
            fight: i8::try_from(*fight).unwrap_or(i8::MAX),
            evade: i8::try_from(*evade).unwrap_or(i8::MAX),
            max_health,
            damage: 0,
            attack_damage: *damage,
            attack_horror: *horror,
            current_location: location,
            exhausted: false,
            engaged_with: None,
            hunter: *hunter,
            prey: *prey,
            retaliate: *retaliate,
            victory: *victory,
            attachments: Vec::new(),
            owner,
        })
    }
}

#[cfg(test)]
mod hunter_prey_field_tests {
    use super::*;
    use crate::test_support;

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
            engaged_with: None,
            hunter: true,
            prey: Prey::Default,
            retaliate: true,
            code: CardCode::new("01116"),
            victory: Some(2),
            attachments: Vec::new(),
            owner: Owner::EncounterDeck,
        };
        assert!(e.hunter);
        assert_eq!(e.prey, Prey::Default);
        assert!(e.retaliate);
    }

    #[test]
    fn test_enemy_fixture_carries_a_code() {
        let e = test_support::test_enemy(7, "Ghoul");
        assert!(
            !e.code.as_str().is_empty(),
            "every enemy carries its printed code"
        );
    }
}
