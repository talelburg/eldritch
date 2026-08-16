//! Where a defeated enemy's card goes (#632), against the real
//! `cards::REGISTRY` — the routing needs `weakness: true` from real corpus
//! metadata, which `game-core` cannot reach by crate direction.
//!
//! `data/rules-reference/rules/glossary/Defeat.md`:
//!
//! > If an enemy has as much or more damage on it as it has health, that enemy
//! > is defeated and placed on the encounter discard pile (or on its owner's
//! > discard pile if it is a weakness).
//!
//! `data/rules-reference/rules/glossary/Encounter_Deck.md`:
//!
//! > If the encounter deck is empty, shuffle the encounter discard pile back
//! > into the encounter deck.
//!
//! `data/rules-reference/rules/glossary/Victory_Display_Victory_Points.md`:
//!
//! > As a victory point enemy is defeated, place the card in the victory
//! > display instead of in the discard pile.
//!
//! The three enemies under test, verbatim from
//! `data/arkhamdb-snapshot/pack/core/`:
//!
//! - **Ghoul Minion 01160** (`core_encounter.json`) — no card text; health 2,
//!   fight 2, no Victory value.
//! - **Mob Enforcer 01101** (`core.json`, `subtype_code: "basicweakness"`) —
//!   "**Prey** - Bearer only.\nHunter.\n\\[action\\] Spend 4 resources:
//!   **Parley.** Discard Mob Enforcer."; health 3, fight 4.
//!   (Its Parley *discards* rather than defeats — <https://arkhamdb.com/card/01101>:
//!   "Discarding an enemy is not the same as defeating it" — so it does not
//!   reach the path under test here.)
//! - **Ghoul Priest 01116** (`core_encounter.json`) — "**Prey** - Highest
//!   \\[combat\\].\nHunter. Retaliate."; printed health 5 *per investigator*
//!   (`"health_per_investigator": true`), fight 4, Victory 2. These tests are
//!   solo, so the spawned health the engine would compute is 5 — the number the
//!   fixture uses.
//!
//! The enemies are built as fixtures rather than spawned from the corpus: what
//! the real registry is needed for here is the `weakness` flag the routing reads
//! (`weakness: true` on 01101, per `crates/cards/src/generated/cards.rs`), which
//! `game-core`'s own tests cannot see.

use game_core::action::{EngineRecord, InputResponse};
use game_core::engine::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, InvestigatorId, LocationId, Phase, TokenModifiers,
};
use game_core::test_support::{
    take_turn_action, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{apply, assert_event, Action, PlayerAction};

const GHOUL_MINION: &str = "01160";
const MOB_ENFORCER: &str = "01101";
const GHOUL_PRIEST: &str = "01116";

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// A solo investigator engaged with `code`, one point of damage short of
/// defeat, with combat high enough that the Fight test cannot fail against the
/// bag's single `Numeric(0)` token. The unarmed Fight deals 1 damage — enough
/// to defeat.
fn solo_investigator_facing(
    code: &str,
    health: u8,
    fight: i8,
    victory: Option<u8>,
) -> (InvestigatorId, EnemyId, game_core::GameState) {
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 8;

    let mut enemy = test_enemy(100, "Enemy under test");
    enemy.code = CardCode::new(code);
    enemy.fight = fight;
    enemy.max_health = health;
    enemy.damage = health - 1;
    enemy.victory = victory;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id); // Fight is location-gated (#401)

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(0)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator_turn(inv_id)
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(test_location(10, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, state)
}

/// Fight the enemy to death: the Fight suspends on the commit window, and
/// committing nothing resolves the test, the damage, and the defeat.
fn fight_to_defeat(
    state: game_core::GameState,
    inv_id: InvestigatorId,
    enemy_id: EnemyId,
) -> game_core::engine::ApplyResult {
    let after_fight = take_turn_action(
        state,
        &TurnAction::Fight {
            investigator: inv_id,
            enemy: enemy_id,
        },
    );
    let result = apply(
        after_fight.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple { selected: vec![] },
        }),
    );
    assert_event!(result.events, Event::EnemyDefeated { enemy: e, .. } if *e == enemy_id);
    assert!(
        !result.state.enemies.contains_key(&enemy_id),
        "the defeated enemy leaves play"
    );
    result
}

#[test]
fn defeated_ghoul_minion_is_drawn_again_once_the_encounter_deck_runs_out() {
    let (inv_id, enemy_id, state) = solo_investigator_facing(GHOUL_MINION, 2, 2, None);
    let after = fight_to_defeat(state, inv_id, enemy_id).state;

    assert_eq!(
        after.encounter_discard,
        vec![CardCode::new(GHOUL_MINION)],
        "a defeated non-weakness enemy is placed on the encounter discard pile"
    );

    // Now run the divergence scenario end to end: the encounter deck is empty
    // and an encounter card is drawn. `draw_encounter_top` reshuffles the
    // discard back in first ("If the encounter deck is empty, shuffle the
    // encounter discard pile back into the encounter deck") — so the card drawn
    // must be the Ghoul the investigator just killed, which then spawns again.
    // Before this fix the discard was empty and the draw had nothing to find.
    let mut state = after;
    assert!(state.encounter_deck.is_empty());
    state.continuations.clear(); // drop the open-turn prompt; draw straight
    state.phase = Phase::Mythos;

    let redraw = apply(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: inv_id,
        }),
    );
    assert_event!(
        redraw.events,
        Event::CardRevealed { code, .. } if code.as_str() == GHOUL_MINION
    );
    assert!(
        redraw
            .state
            .enemies
            .values()
            .any(|e| e.code.as_str() == GHOUL_MINION),
        "the reshuffled Ghoul Minion spawns again; enemies = {:?}",
        redraw.state.enemies
    );
    assert!(redraw.state.encounter_discard.is_empty());
}

#[test]
fn defeated_enemy_weakness_lands_in_its_owners_discard_pile() {
    let (inv_id, enemy_id, state) = solo_investigator_facing(MOB_ENFORCER, 3, 4, None);
    let after = fight_to_defeat(state, inv_id, enemy_id).state;

    let inv = &after.investigators[&inv_id];
    assert_eq!(
        inv.discard,
        vec![CardCode::new(MOB_ENFORCER)],
        "a defeated enemy weakness goes to its owner's discard pile, so it stays \
         part of that investigator's deck for the campaign"
    );
    assert!(
        after.encounter_discard.is_empty(),
        "…and NOT to the encounter discard, which would feed a player card into \
         the encounter deck on the next reshuffle"
    );
    assert!(
        !inv.removed_from_game.contains(&CardCode::new(MOB_ENFORCER)),
        "…and NOT out of the game"
    );
    assert!(after.victory_display.is_empty());
}

#[test]
fn defeated_victory_enemy_goes_to_the_victory_display_and_no_discard_pile() {
    let (inv_id, enemy_id, state) = solo_investigator_facing(GHOUL_PRIEST, 5, 4, Some(2));
    let after = fight_to_defeat(state, inv_id, enemy_id).state;

    assert_eq!(after.victory_display, vec![CardCode::new(GHOUL_PRIEST)]);
    assert!(
        after.encounter_discard.is_empty(),
        "the victory display is instead of the discard pile"
    );
    assert!(after.investigators[&inv_id].discard.is_empty());
}
