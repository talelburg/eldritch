use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;

mod classify;
mod emit_card;
mod map_card_type;
mod map_class;
mod normalize;
mod other;
mod parse_traits;
mod prey;
mod process_raw;
mod slots;

/// Minimal `RawCard` fixture with the required fields populated and
/// optional fields cleared. Tweak by mutating the returned value
/// before passing to `normalize` / `process_raw`.
fn raw_card(code: &str) -> RawCard {
    RawCard {
        code: code.to_owned(),
        name: Some(format!("Card {code}")),
        text: None,
        traits: None,
        slot: None,
        cost: None,
        xp: None,
        health: None,
        sanity: None,
        deck_limit: None,
        quantity: None,
        back_name: None,
        back_text: None,
        pack_code: "core".to_owned(),
        faction_code: Some("seeker".to_owned()),
        type_code: Some("asset".to_owned()),
        skill_willpower: None,
        skill_intellect: None,
        skill_combat: None,
        skill_agility: None,
        skill_wild: None,
        shroud: None,
        clues: None,
        clues_fixed: None,
        victory: None,
        doom: None,
        enemy_fight: None,
        enemy_evade: None,
        enemy_damage: None,
        enemy_horror: None,
        health_per_investigator: None,
        subtype_code: None,
    }
}
