//! Roland Banks (01001) reacts from a **roster-seated** investigator — his
//! `[reaction]` fires with NO manual `cards_in_play` injection, sourced from
//! the investigator card via the unified `controlled_card_instances()` scan
//! (#448 cp3a). Caps once per round through `investigator_card.ability_usage`.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01001):
//! > [reaction] After you defeat an enemy: Discover 1 clue at your
//! > location. (Limit once per round.)
//!
//! Integration test so it can install `cards::REGISTRY` in its own process.

use cards::REGISTRY;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    AbilityUsageRecord, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    LocationId, TokenModifiers,
};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event, assert_no_event};

const ROLAND: &str = "01001";

const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Roland, seated from his printed card (Combat 4), engaged with a 1-HP enemy.
/// His investigator card is the seat's own — `cards_in_play` is empty, proving
/// the reaction is found by the investigator-card scan, not the in-play scan.
fn seated_roland_with_enemy() -> GameState {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Mock Ghoul");
    enemy.fight = 1;
    enemy.max_health = 1;

    let mut loc = test_support::test_location(HERE.0, "Study");
    loc.clues = 2;

    let state = GameStateBuilder::new()
        .with_seat(seat(1).as_card(ROLAND).at(HERE))
        .with_enemy_engaged(enemy, INV)
        .with_location(loc)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .build();
    assert!(
        state.investigators[&INV].cards_in_play.is_empty(),
        "seated shape: no in-play injection"
    );
    state
}

/// Fight the enemy from `state`, committing nothing.
fn fight(state: GameState) -> TestSession {
    TestSession::new(state)
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: ENEMY,
        })
        .commit(&[])
}

#[test]
fn seated_roland_reaction_fires_with_no_in_play_injection() {
    let at_window = fight(seated_roland_with_enemy());
    // The reaction is offered on Roland's investigator card.
    let rolands_card = OptionTarget::CardInstance(
        at_window.state().investigators[&INV]
            .investigator_card
            .instance_id,
    );
    let session = at_window.pick(rolands_card);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == ENEMY && *by == INV
    );
    assert_event!(
        session.events(),
        Event::CluePlaced { investigator, count: 1 } if *investigator == INV
    );
    assert_eq!(session.state().locations[&HERE].clues, 1);
    assert_eq!(session.state().investigators[&INV].clues, 1);

    // Usage bumped on the investigator CARD (#448 cp3a — the unified scan fires
    // it via `controlled_card_instances()`, so usage lives on the card's own
    // `ability_usage`, not `Investigator.ability_usage`): ability index 0, round 0.
    let inv = &session.state().investigators[&INV];
    assert_eq!(
        inv.investigator_card.ability_usage.get(&0),
        Some(&AbilityUsageRecord::new(0, 1)),
        "seated Roland's reaction recorded one fire on investigator_card.ability_usage",
    );
}

#[test]
fn seated_roland_reaction_capped_once_per_round() {
    let mut state = seated_roland_with_enemy();
    // Pretend Roland already reacted this round — bump the investigator CARD's
    // usage (#448 cp3a: the unified scan reads `investigator_card.ability_usage`).
    state
        .investigators
        .get_mut(&INV)
        .unwrap()
        .investigator_card
        .bump_ability_usage(0, 0);

    let session = fight(state);

    // No reaction window: the commit runs straight back to the menu.
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::EnemyDefeated { enemy: e, by: Some(by) } if *e == ENEMY && *by == INV
    );
    // Limit exhausted → no second reaction → no clue moved.
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(session.state().locations[&HERE].clues, 2);
}
