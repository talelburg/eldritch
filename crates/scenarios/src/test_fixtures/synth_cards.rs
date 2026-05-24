//! Synthetic test cards used by Phase-4's integration tests.
//!
//! These don't exist in any printed pack — they're vehicles for
//! proving engine wiring end-to-end without depending on real corpus
//! cards. The card codes use an underscore prefix (`_synth_*`) to
//! guarantee no collision with `ArkhamDB`'s digit-prefixed codes.
//!
//! Exposed alongside [`TEST_REGISTRY`] — integration tests install
//! this registry so the on-draw path resolves against synthetic cards
//! that are guaranteed not to collide with real `ArkhamDB` codes
//! (underscore-prefix), rather than depending on a specific corpus
//! card existing. The `cards` crate is still compiled in as a
//! workspace dep — what `TEST_REGISTRY` isolates is the *runtime*
//! registry lookup, not the compile-time footprint.

use std::sync::OnceLock;

use game_core::card_data::{CardMetadata, CardType, Class, SkillIcons, Spawn, SpawnLocation};
use game_core::card_registry::CardRegistry;
use game_core::dsl::{gain_resources, revelation, Ability, InvestigatorTarget};
use game_core::state::CardCode;

/// Code for the synthetic location used by the synth-enemy's spawn
/// rule. Underscore prefix guarantees no collision with
/// `ArkhamDB`'s digit-prefixed real codes. Referenced from
/// [`crate::test_fixtures::synthetic::setup`] when stamping the demo
/// location's `code` field.
pub const SYNTH_LOC_CODE: &str = "_synth_loc";

/// Code for the synthetic spawn-bearing enemy.
///
/// Carries `SpawnLocation::Specific(SYNTH_LOC_CODE)` so the on-draw
/// path's enemy arm has something to spawn during the integration
/// test in `crates/scenarios/tests/encounter_spawn.rs`. No abilities
/// (no Revelation, no Activated triggers) — the proof we need is
/// "enemy spawns at the right location, engages the right
/// investigator," not anything ability-driven.
pub const SYNTH_ENEMY_CODE: &str = "_synth_enemy";

/// Code for the synthetic treachery. Underscore prefix guarantees no
/// collision with `ArkhamDB`'s digit-prefixed five-char codes.
pub const SYNTH_TREACHERY_CODE: &str = "_synth_treachery";

/// Static metadata for the synthetic treachery. Fields populated with
/// trivial defaults — only `code`, `name`, `card_type`, and
/// `deck_limit`/`quantity` carry meaning for the tests; the rest
/// satisfy `CardMetadata`'s non-`#[non_exhaustive]` struct shape.
fn synth_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_TREACHERY_CODE.to_owned(),
        name: "Synthetic Treachery".to_owned(),
        class: Class::Mythos,
        card_type: CardType::Treachery,
        cost: None,
        xp: None,
        text: Some("Revelation - You gain 1 resource. (Synthetic; not a printed card.)".to_owned()),
        flavor: None,
        illustrator: None,
        traits: Vec::new(),
        slots: Vec::new(),
        skill_icons: SkillIcons {
            willpower: 0,
            intellect: 0,
            combat: 0,
            agility: 0,
            wild: 0,
        },
        health: None,
        sanity: None,
        deck_limit: 1,
        quantity: 1,
        pack_code: "_synth".to_owned(),
        position: 1,
        is_fast: false,
        spawn: None,
        surge: false,
        peril: false,
    }
}

fn synth_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_treachery_metadata)
}

fn synth_enemy_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_ENEMY_CODE.to_owned(),
        name: "Synthetic Enemy".to_owned(),
        class: Class::Mythos,
        card_type: CardType::Enemy,
        cost: None,
        xp: None,
        text: Some("Spawn: Synthetic Location. (Synthetic; not a printed card.)".to_owned()),
        flavor: None,
        illustrator: None,
        traits: Vec::new(),
        slots: Vec::new(),
        skill_icons: SkillIcons {
            willpower: 0,
            intellect: 0,
            combat: 0,
            agility: 0,
            wild: 0,
        },
        health: Some(1),
        sanity: None,
        deck_limit: 1,
        quantity: 1,
        pack_code: "_synth".to_owned(),
        position: 2,
        is_fast: false,
        spawn: Some(Spawn {
            location: SpawnLocation::Specific(SYNTH_LOC_CODE.to_owned()),
        }),
        surge: false,
        peril: false,
    }
}

fn synth_enemy_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_enemy_metadata)
}

/// `metadata_for` function pointer used by [`TEST_REGISTRY`].
fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(synth_treachery_metadata_static()),
        SYNTH_ENEMY_CODE => Some(synth_enemy_metadata_static()),
        _ => None,
    }
}

/// `abilities_for` function pointer used by [`TEST_REGISTRY`].
fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(vec![revelation(gain_resources(
            InvestigatorTarget::Controller,
            1,
        ))]),
        // SYNTH_ENEMY_CODE intentionally returns None — the synthetic
        // enemy has no Revelation effect; the spawn handler is the
        // only thing exercised by the integration test.
        _ => None,
    }
}

/// Ready-made [`CardRegistry`] backed by this module's synthetic
/// cards. Integration tests install this via
/// [`game_core::card_registry::install`] instead of `cards::REGISTRY`
/// so they don't pull in the full corpus.
///
/// Process-isolated: each `cargo test --test` binary gets its own
/// process, so this install doesn't collide with `cards::REGISTRY`
/// installs in other test binaries.
pub const TEST_REGISTRY: CardRegistry = CardRegistry {
    metadata_for,
    abilities_for,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_for_resolves_synth_treachery() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let meta = metadata_for(&code).expect("synth treachery must resolve");
        assert_eq!(meta.code, SYNTH_TREACHERY_CODE);
        assert_eq!(meta.card_type, CardType::Treachery);
    }

    #[test]
    fn metadata_for_returns_none_for_unknown_code() {
        let code = CardCode("not_in_synth_registry".into());
        assert!(metadata_for(&code).is_none());
    }

    #[test]
    fn abilities_for_returns_one_revelation_ability() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let abilities = abilities_for(&code).expect("synth treachery must have abilities");
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0].trigger, game_core::dsl::Trigger::Revelation,);
    }

    #[test]
    fn test_registry_dispatches_to_module_functions() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        assert!((TEST_REGISTRY.metadata_for)(&code).is_some());
        assert!((TEST_REGISTRY.abilities_for)(&code).is_some());
    }

    #[test]
    fn metadata_for_resolves_synth_enemy() {
        let code = CardCode(SYNTH_ENEMY_CODE.into());
        let meta = metadata_for(&code).expect("synth enemy must resolve");
        assert_eq!(meta.code, SYNTH_ENEMY_CODE);
        assert_eq!(meta.card_type, game_core::card_data::CardType::Enemy);
        let spawn = meta
            .spawn
            .as_ref()
            .expect("synth enemy must carry a spawn rule");
        match &spawn.location {
            game_core::card_data::SpawnLocation::Specific(code) => {
                assert_eq!(code, SYNTH_LOC_CODE);
            }
        }
    }

    #[test]
    fn abilities_for_synth_enemy_returns_none() {
        let code = CardCode(SYNTH_ENEMY_CODE.into());
        assert!(abilities_for(&code).is_none());
    }
}
