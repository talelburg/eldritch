//! An enemy encounter card spawns via the disposition frame (Slice D, #423).
//!
//! `resolve_encounter_card`'s enemy arm no longer spawns inline — it pushes a
//! [`Continuation::EncounterCard`] frame carrying
//! [`EncounterDisposition::Spawn`], and the global `drive` loop spawns the enemy
//! when it disposes of that frame after the (here empty) Revelation. This test
//! drives an `EncounterCardRevealed` engine record through the real `apply`
//! loop and proves the enemy lands in play with no `EncounterCard` frame left
//! behind.
//!
//! Uses a process-local mock registry (the `round_end_rescan.rs` pattern): a
//! synthetic enemy `CardMetadata` with `spawn: None` (spawns at the drawer's
//! location) and no abilities (no Revelation).

use game_core::action::{Action, EngineRecord};
use game_core::card_data::{CardKind, CardMetadata, HealthValue, Prey};
use game_core::card_registry::{self, CardRegistry, NativeEffectFn};
use game_core::dsl::Ability;
use game_core::state::{CardCode, Continuation, InvestigatorId, LocationId};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::EngineOutcome;
use std::sync::OnceLock;

const ENEMY: &str = "_synth_enemy";

fn synth_enemy_metadata() -> &'static CardMetadata {
    static META: OnceLock<CardMetadata> = OnceLock::new();
    META.get_or_init(|| CardMetadata {
        code: ENEMY.into(),
        name: "Synth Enemy".into(),
        text: None,
        traits: Vec::new(),
        pack_code: "_synth".into(),
        weakness: false,
        kind: CardKind::Enemy {
            fight: 1,
            evade: 1,
            damage: 0,
            horror: 0,
            health: Some(HealthValue::Fixed(1)),
            victory: None,
            spawn: None,
            surge: false,
            peril: false,
            hunter: false,
            retaliate: false,
            prey: Prey::Default,
            quantity: 1,
        },
    })
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    (code.as_str() == ENEMY).then(synth_enemy_metadata)
}

fn mock_abilities_for(_: &CardCode) -> Option<Vec<Ability>> {
    None
}

fn mock_native_for(_: &str) -> Option<NativeEffectFn> {
    None
}

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: mock_native_for,
        native_eligibility_for: |_| None,
    });
}

#[test]
fn enemy_encounter_card_spawns_via_the_disposition_frame() {
    let mut state = GameStateBuilder::new()
        .with_investigator_at(test_investigator(1), LocationId(1))
        .with_location(test_location(1, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.encounter_deck.push_back(CardCode::new(ENEMY));

    let result = drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        {
            let mut r = ScriptedResolver::new();
            r.commit_cards(&[]);
            r
        },
    );

    assert_eq!(
        result.outcome,
        EngineOutcome::Done,
        "spawn resolves cleanly"
    );
    assert_eq!(
        result.state.enemies.len(),
        1,
        "the enemy spawned into play via the disposition frame",
    );
    let enemy = result.state.enemies.values().next().expect("enemy present");
    assert_eq!(
        enemy.current_location,
        Some(LocationId(1)),
        "spawned at the drawer's location (spawn: None)",
    );
    assert!(
        !result
            .state
            .continuations
            .iter()
            .any(|c| matches!(c, Continuation::EncounterCard { .. })),
        "no EncounterCard frame remains after disposal",
    );
}
