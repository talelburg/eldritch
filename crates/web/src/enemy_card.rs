//! Visual rendering of enemies for the web client.
//!
//! Enemies are a distinct data source — the `Enemy` state struct carries stats
//! *and* live state, while its traits and text stay on the printed card — so
//! they get a dedicated `EnemyCard` component rather than reusing `Card` (which
//! is built around registry lookup + a `CardInPlay`). Shares the card CSS /
//! chip vocabulary and the text renderer. Display-only.

use game_core::engine::OptionTarget;
use game_core::state::Enemy;
use leptos::prelude::*;

use crate::interaction::{self, PendingOptions};
use crate::{card, registry};

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
/// the card CSS and the `card--exhausted` dim the asset cards use. Stats and
/// live state come from the `Enemy` state struct; traits and ability text come
/// from its printed card, looked up by code via the registry.
/// Display-only — no click handlers.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Leptos `#[component]` takes `enemy` by value into its owned props struct"
)]
#[component]
pub fn EnemyCard(enemy: Enemy) -> impl IntoView {
    let name = enemy.name.clone();
    let meta = (registry::use_cards().metadata_for)(&enemy.code);
    let traits = match meta {
        Some(m) if !m.traits.is_empty() => format!("{}.", m.traits.join(". ")),
        _ => String::new(),
    };
    let text_view = meta
        .and_then(|m| m.text.as_deref())
        .map(|t| card::render_segments(card::parse_card_text(t)));
    let exhausted = enemy.exhausted;
    let exhausted_badge =
        exhausted.then(|| view! { <span class="card-exhausted">"Exhausted"</span> });
    // Interactivity: route the live prompt's options to this enemy; glow +
    // open a context menu of its offered actions (Fight/Evade) when non-empty.
    let pending = use_context::<PendingOptions>()
        .map(|p| p.0.get())
        .unwrap_or_default();
    let menu_opts = interaction::options_for(&pending, &OptionTarget::Enemy(enemy.id));
    let actionable = !menu_opts.is_empty();
    #[cfg(target_arch = "wasm32")]
    let open = RwSignal::new(None::<(i32, i32)>);
    let stat_views: Vec<_> = enemy_stat_chips(&enemy)
        .into_iter()
        .map(|s| view! { <span class="chip chip--enemy-stat">{s}</span> })
        .collect();
    let keyword_views: Vec<_> = enemy_keyword_chips(&enemy)
        .into_iter()
        .map(|s| view! { <span class="chip chip--keyword">{s}</span> })
        .collect();
    let mut root_class = String::from("card card--enemy");
    if exhausted {
        root_class.push_str(" card--exhausted");
    }
    if actionable {
        root_class.push_str(" actionable");
    }
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
            {
                // wasm-only trigger + menu (web_sys / OutboundTx); host: empty,
                // `menu_opts` used above by `actionable` (no unused-var warning).
                #[cfg(target_arch = "wasm32")]
                actionable.then(|| interaction::menu_layer(menu_opts, open))
            }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use game_core::test_support::{enemy, TEST_REGISTRIES};

    use super::*;

    #[test]
    fn stat_chips_in_order() {
        let e = enemy(1)
            .fight(3)
            .evade(2)
            .health(3)
            .damage(1)
            .attack_damage(2)
            .attack_horror(1)
            .mint(TEST_REGISTRIES, 1);
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
        let e = enemy(1)
            .hunter()
            .retaliate()
            .victory(2)
            .mint(TEST_REGISTRIES, 1);
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
        let e = enemy(2).mint(TEST_REGISTRIES, 1);
        assert!(enemy_keyword_chips(&e).is_empty());
    }
}
