//! Roland Banks (01001) reacts from a **roster-seated** investigator — his
//! `[reaction]` fires with NO manual `cards_in_play` injection, sourced from
//! `Investigator.card_code` via the new `scan_investigator_card_reactions`.
//! Caps once per round through `Investigator.ability_usage`.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01001):
//! > [reaction] After you defeat an enemy: Discover 1 clue at your
//! > location. (Limit once per round.)
//!
//! Integration test so it can install `cards::REGISTRY` in its own process.

use std::sync::Once;

use game_core::engine::{EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    AbilityUsageRecord, ChaosBag, ChaosToken, EnemyId, InvestigatorId, LocationId, Phase,
    TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, drive, test_enemy, test_investigator, test_location, GameStateBuilder,
    ScriptedResolver,
};
use game_core::{assert_event, assert_no_event, Action, PlayerAction};

const ROLAND: &str = "01001";

fn install_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Roland engaged with a 1-HP enemy, his investigator card represented ONLY by
/// `card_code` (the seated shape) — `cards_in_play` is empty, proving the
/// reaction is found by the new investigator-card scan, not the in-play scan.
fn seated_roland_with_enemy(
    round: u32,
) -> (InvestigatorId, EnemyId, LocationId, game_core::GameState) {
    install_registry();
    let inv_id = InvestigatorId(1);
    let enemy_id = EnemyId(100);
    let loc_id = LocationId(10);

    let mut inv = test_investigator(1);
    inv.card_code = game_core::state::CardCode::new(ROLAND);
    inv.current_location = Some(loc_id);
    inv.skills.combat = 4;
    assert!(
        inv.cards_in_play.is_empty(),
        "seated shape: no in-play injection"
    );

    let mut enemy = test_enemy(100, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;
    enemy.damage = 0;
    enemy.engaged_with = Some(inv_id);
    enemy.current_location = Some(loc_id);

    let mut loc = test_location(10, "Study");
    loc.clues = 2;

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_round(round)
        .with_active_investigator(inv_id)
        .with_turn_order([inv_id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (inv_id, enemy_id, loc_id, state)
}

fn fight_action(inv: InvestigatorId, enemy: EnemyId) -> Action {
    Action::Player(PlayerAction::Fight {
        investigator: inv,
        enemy,
    })
}

#[test]
fn seated_roland_reaction_fires_with_no_in_play_injection() {
    let (inv_id, enemy_id, loc_id, state) = seated_roland_with_enemy(0);

    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]).pick_single(OptionId(0));
    let result = drive(state, fight_action(inv_id, enemy_id), resolver);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    assert_event!(
        result.events,
        Event::CluePlaced { investigator, count: 1 } if *investigator == inv_id
    );
    assert_eq!(result.state.locations[&loc_id].clues, 1);
    assert_eq!(result.state.investigators[&inv_id].clues, 1);

    // Usage bumped on the INVESTIGATOR (not a CardInPlay): ability index 0, round 0.
    let inv = &result.state.investigators[&inv_id];
    assert_eq!(
        inv.ability_usage.get(&0),
        Some(&AbilityUsageRecord::new(0, 1)),
        "seated Roland's reaction recorded one fire on Investigator.ability_usage",
    );
}

#[test]
fn seated_roland_reaction_capped_once_per_round() {
    let (inv_id, enemy_id, loc_id, mut state) = seated_roland_with_enemy(0);
    // Pretend Roland already reacted this round.
    state
        .investigators
        .get_mut(&inv_id)
        .unwrap()
        .bump_ability_usage(0, 0);

    let result = apply_no_commits(state, fight_action(inv_id, enemy_id));

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(
        result.events,
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == enemy_id && *by == inv_id
    );
    // Limit exhausted → no second reaction → no clue moved.
    assert_no_event!(result.events, Event::CluePlaced { .. });
    assert_eq!(result.state.locations[&loc_id].clues, 2);
}
