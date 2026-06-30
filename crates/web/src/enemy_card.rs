//! Visual rendering of enemies for the web client. Enemies are a distinct data
//! source — the `Enemy` state struct carries stats *and* live state — so they
//! get a dedicated `EnemyCard` component rather than reusing `Card` (which is
//! built around registry lookup + a `CardInPlay`). Shares the card CSS / chip
//! vocabulary and the text renderer. Display-only.

use game_core::state::Enemy;

/// Combat stat chips for an enemy: fight, evade, health (damage/max), attack
/// (damage + horror), in that order.
#[must_use]
pub fn enemy_stat_chips(enemy: &Enemy) -> Vec<String> {
    vec![
        format!("fight {}", enemy.fight),
        format!("evade {}", enemy.evade),
        format!("health {}/{}", enemy.damage, enemy.max_health),
        format!(
            "attack: {} dmg · {} hor",
            enemy.attack_damage, enemy.attack_horror
        ),
    ]
}

/// Keyword / victory chips present on an enemy: `"Hunter"`, `"Retaliate"`,
/// `"Victory {n}"` — only those that apply, in that order.
#[must_use]
pub fn enemy_keyword_chips(enemy: &Enemy) -> Vec<String> {
    let mut chips = Vec::new();
    if enemy.hunter {
        chips.push("Hunter".to_string());
    }
    if enemy.retaliate {
        chips.push("Retaliate".to_string());
    }
    if let Some(n) = enemy.victory {
        chips.push(format!("Victory {n}"));
    }
    chips
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_core::test_support::fixtures::test_enemy;

    #[test]
    fn stat_chips_in_order() {
        let mut e = test_enemy(1, "Ghoul");
        e.fight = 3;
        e.evade = 2;
        e.max_health = 3;
        e.damage = 1;
        e.attack_damage = 1;
        e.attack_horror = 1;
        assert_eq!(
            enemy_stat_chips(&e),
            vec![
                "fight 3".to_string(),
                "evade 2".to_string(),
                "health 1/3".to_string(),
                "attack: 1 dmg · 1 hor".to_string(),
            ]
        );
    }

    #[test]
    fn keyword_chips_only_when_present() {
        let mut e = test_enemy(1, "Ghoul Priest");
        e.hunter = true;
        e.retaliate = true;
        e.victory = Some(2);
        assert_eq!(
            enemy_keyword_chips(&e),
            vec![
                "Hunter".to_string(),
                "Retaliate".to_string(),
                "Victory 2".to_string(),
            ]
        );
    }

    #[test]
    fn keyword_chips_empty_for_plain_enemy() {
        let e = test_enemy(2, "Swarm of Rats");
        assert!(enemy_keyword_chips(&e).is_empty());
    }
}
