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

use game_core::card_data::{
    CardKind, CardMetadata, Class, HealthValue, Prey, SkillIcons, Spawn, SpawnLocation,
};
use game_core::card_registry::CardRegistry;
use game_core::dsl::{gain_resources, on_play, revelation, Ability, InvestigatorTarget};
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

/// Code for the synthetic surge-bearing treachery. Its Revelation
/// is the same trivial "gain 1 resource" as [`SYNTH_TREACHERY_CODE`];
/// the load-bearing difference is `surge: true` on the metadata,
/// which drives the surge re-draw path in the per-card sub-sequence
/// (Rules Reference p.19, p.24 1.4 step 5).
pub const SYNTH_SURGE_TREACHERY_CODE: &str = "_synth_surge_treachery";

/// Code for the synthetic Fast event. Used to test the `MythosAfterDraws`
/// window's push-then-scan ordering fix: a Fast event in hand during
/// Mythos must keep the window open (not auto-skip) and must be
/// closeable via `ResolveInput::Skip` after playing (or without
/// playing, per the player's choice).
///
/// The card's `OnPlay` effect is trivially "gain 1 resource" — the
/// effect itself is unimportant; what matters is `is_fast: true` and
/// `card_type: Event` so `check_play_card`'s timing gate allows it
/// inside a permissive window.
pub const SYNTH_FAST_EVENT_CODE: &str = "_synth_fast_event";

/// Static metadata for the synthetic treachery. Only `code`/`name`/the
/// `Treachery` kind carry meaning for the tests.
fn synth_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_TREACHERY_CODE.to_owned(),
        name: "Synthetic Treachery".to_owned(),
        text: Some("Revelation - You gain 1 resource. (Synthetic; not a printed card.)".to_owned()),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        kind: CardKind::Treachery {
            surge: false,
            peril: false,
            quantity: 1,
        },
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
        text: Some("Spawn: Synthetic Location. (Synthetic; not a printed card.)".to_owned()),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        kind: CardKind::Enemy {
            fight: 1,
            evade: 1,
            damage: 0,
            horror: 0,
            health: Some(HealthValue::Fixed(1)),
            victory: None,
            spawn: Some(Spawn {
                location: SpawnLocation::Specific(SYNTH_LOC_CODE.to_owned()),
            }),
            surge: false,
            peril: false,
            hunter: false,
            retaliate: false,
            prey: Prey::Default,
            quantity: 1,
        },
    }
}

fn synth_enemy_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_enemy_metadata)
}

fn synth_surge_treachery_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_SURGE_TREACHERY_CODE.to_owned(),
        name: "Synthetic Surge Treachery".to_owned(),
        text: Some(
            "Revelation - You gain 1 resource. Surge. \
             (Synthetic; not a printed card.)"
                .to_owned(),
        ),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        kind: CardKind::Treachery {
            surge: true,
            peril: false,
            quantity: 1,
        },
    }
}

fn synth_surge_treachery_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_surge_treachery_metadata)
}

fn synth_fast_event_metadata() -> CardMetadata {
    CardMetadata {
        code: SYNTH_FAST_EVENT_CODE.to_owned(),
        name: "Synthetic Fast Event".to_owned(),
        text: Some(
            "Fast. Play at any player window. \
             You gain 1 resource. (Synthetic; not a printed card.)"
                .to_owned(),
        ),
        traits: Vec::new(),
        pack_code: "_synth".to_owned(),
        kind: CardKind::Event {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            skill_icons: SkillIcons::default(),
            is_fast: true,
            deck_limit: 3,
        },
    }
}

fn synth_fast_event_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(synth_fast_event_metadata)
}

/// `metadata_for` function pointer used by [`TEST_REGISTRY`].
fn metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE => Some(synth_treachery_metadata_static()),
        SYNTH_ENEMY_CODE => Some(synth_enemy_metadata_static()),
        SYNTH_SURGE_TREACHERY_CODE => Some(synth_surge_treachery_metadata_static()),
        SYNTH_FAST_EVENT_CODE => Some(synth_fast_event_metadata_static()),
        _ => None,
    }
}

/// `abilities_for` function pointer used by [`TEST_REGISTRY`].
fn abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SYNTH_TREACHERY_CODE | SYNTH_SURGE_TREACHERY_CODE => {
            Some(vec![revelation(gain_resources(InvestigatorTarget::You, 1))])
        }
        SYNTH_FAST_EVENT_CODE => Some(vec![on_play(gain_resources(InvestigatorTarget::You, 1))]),
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
    native_effect_for: |_| None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_for_resolves_synth_treachery() {
        let code = CardCode(SYNTH_TREACHERY_CODE.into());
        let meta = metadata_for(&code).expect("synth treachery must resolve");
        assert_eq!(meta.code, SYNTH_TREACHERY_CODE);
        assert_eq!(meta.card_type(), game_core::card_data::CardType::Treachery);
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
        assert_eq!(meta.card_type(), game_core::card_data::CardType::Enemy);
        let CardKind::Enemy { spawn, .. } = &meta.kind else {
            panic!("synth enemy must be an Enemy kind");
        };
        let spawn = spawn.as_ref().expect("synth enemy must carry a spawn rule");
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
