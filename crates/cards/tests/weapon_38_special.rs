//! C5c (#238) integration: Roland's .38 Special 01006 end-to-end against
//! the real `cards::REGISTRY`. Verifies the clue-conditional combat
//! modifier (+3 with a clue on the location, +1 without) and ammo spend.
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::engine::EngineOutcome;
use game_core::engine::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, InvestigatorId, Phase,
    TokenModifiers, UseKind,
};
use game_core::test_support::{test_enemy, test_investigator, GameStateBuilder, TestSession};
use game_core::{assert_event, assert_no_event};

const SPECIAL: &str = "01006";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: game_core::state::LocationId = game_core::state::LocationId(10);
const ENEMY: EnemyId = EnemyId(100);
const WEAPON_INST: CardInstanceId = CardInstanceId(0);

#[ctor::ctor]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Board: .38 Special in play with 4 ammo, the active investigator (combat
/// 3) at a location holding `loc_clues`, engaged with a fight-**5** enemy
/// (health 3). Fight 5 distinguishes the modifier branches: +3 → total 8
/// hits, +1 → total 4 misses. A `Numeric(0)` chaos bag is deterministic.
fn board(loc_clues: u8) -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.skills.combat = 3;
    let mut weapon = CardInPlay::enter_play(CardCode::new(SPECIAL), WEAPON_INST);
    weapon.uses.insert(UseKind::Ammo, 4);
    inv.cards_in_play.push(weapon);

    let mut enemy = test_enemy(100, "Ghoul");
    enemy.fight = 5;
    enemy.max_health = 3;
    enemy.engaged_with = Some(INV);
    enemy.current_location = Some(LOC); // co-located: a weapon Fight targets enemies at your location

    let mut location = game_core::test_support::test_location(10, "Study");
    location.clues = loc_clues;

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(location)
        .with_enemy(enemy)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

fn ammo(state: &game_core::GameState) -> u8 {
    state.investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == WEAPON_INST)
        .and_then(|c| c.uses.get(&UseKind::Ammo).copied())
        .expect("weapon carries an ammo pool")
}

fn fire(state: game_core::GameState) -> game_core::engine::ApplyResult {
    TestSession::new(state)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            instance_id: WEAPON_INST,
            ability_index: 0,
        })
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .run()
}

#[test]
fn plus_three_with_a_clue_on_location_hits_for_two() {
    // Clue present → +3: combat 3 + 3 = 6 vs fight 5 → success, 1 + 1 = 2.
    let r = fire(board(1));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_event!(r.events, Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(r.state.enemies[&ENEMY].damage, 2);
    assert_eq!(ammo(&r.state), 3, "1 ammo spent");
}

#[test]
fn plus_one_without_a_clue_misses() {
    // No clue → +1: combat 3 + 1 = 4 vs fight 5 → fail, no damage. Ammo is
    // still spent on activation regardless of the result.
    let r = fire(board(0));
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_event!(r.events, Event::SkillTestFailed { .. });
    assert_no_event!(r.events, Event::EnemyDamaged { .. });
    assert_eq!(r.state.enemies[&ENEMY].damage, 0);
    assert_eq!(ammo(&r.state), 3, "ammo spent even on a miss");
}
