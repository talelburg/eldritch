//! Visual rendering of enemies for the web client. Enemies are a distinct data
//! source — the `Enemy` state struct carries stats *and* live state — so they
//! get a dedicated `EnemyCard` component rather than reusing `Card` (which is
//! built around registry lookup + a `CardInPlay`). Shares the card CSS / chip
//! vocabulary and the text renderer. Display-only.

use game_core::state::Enemy;
use leptos::prelude::*;

use crate::card::{parse_card_text, render_segments};

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

/// One engaged enemy rendered as a card (red border via `card--enemy`), reusing
/// the card CSS and the `card--exhausted` dim from the asset slice. Reads from
/// the `Enemy` state struct; ability text is looked up by code via the registry.
/// Display-only — no click handlers.
// `enemy` is taken by value: Leptos `#[component]` generates a props struct
// requiring owned fields, so a reference would need a lifetime the macro can't
// express.
#[allow(clippy::needless_pass_by_value)]
#[component]
pub fn EnemyCard(enemy: Enemy) -> impl IntoView {
    let name = enemy.name.clone();
    let traits = if enemy.traits.is_empty() {
        String::new()
    } else {
        format!("{}.", enemy.traits.join(". "))
    };
    let text_view = game_core::card_registry::current()
        .and_then(|r| (r.metadata_for)(&enemy.code))
        .and_then(|m| m.text.as_deref())
        .map(|t| render_segments(parse_card_text(t)));
    let exhausted = enemy.exhausted;
    let exhausted_badge =
        exhausted.then(|| view! { <span class="card-exhausted">"Exhausted"</span> });
    let stat_views: Vec<_> = enemy_stat_chips(&enemy)
        .into_iter()
        .map(|s| view! { <span class="chip chip--enemy-stat">{s}</span> })
        .collect();
    let keyword_views: Vec<_> = enemy_keyword_chips(&enemy)
        .into_iter()
        .map(|s| view! { <span class="chip chip--keyword">{s}</span> })
        .collect();
    let root_class = if exhausted {
        "card card--enemy card--exhausted"
    } else {
        "card card--enemy"
    };
    view! {
        <div class=root_class>
            <div class="card-head">
                <span class="card-name">{name}</span>
                {exhausted_badge}
            </div>
            <div class="card-traits">{traits}</div>
            <div class="card-text">{text_view}</div>
            <div class="card-footer enemy-stats">
                {stat_views}
                {keyword_views}
            </div>
        </div>
    }
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
        e.attack_damage = 2;
        e.attack_horror = 1;
        assert_eq!(
            enemy_stat_chips(&e),
            vec![
                "fight 3".to_string(),
                "evade 2".to_string(),
                "health 1/3".to_string(),
                "attack: 2 dmg · 1 hor".to_string(),
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
