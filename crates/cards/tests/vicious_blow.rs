//! Vicious Blow 01025 end-to-end against the real
//! `cards::REGISTRY`. The Guardian L0 skill commits to a Fight test and,
//! on success, adds +1 to the attack's damage via its `OnCommit`
//! `BoostAttackDamage(1)` ability.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId, LocationId, TokenModifiers,
};
use game_core::test_support::{self, seat, TestSession};

const VICIOUS_BLOW: &str = "01025";
const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

/// The controller (combat 3) engaged with one enemy (fight 2, health 10 so the
/// dealt damage is observable, not clamped), Vicious Blow in hand, a
/// `Numeric(0)` chaos bag for a deterministic success, fighting it and
/// committing `commit`.
fn fight_committing(commit: &[&str]) -> TestSession {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
    enemy.fight = 2;
    enemy.max_health = 10;

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).hand([VICIOUS_BLOW]))
        .with_location(test_support::test_location(HERE.0, "Study"))
        // Co-located: Fight is location-gated.
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: ENEMY,
        })
        .commit(commit)
}

/// Committing Vicious Blow to a successful Fight deals `1 base + 1 = 2`.
#[test]
fn committing_vicious_blow_adds_one_attack_damage() {
    let session = fight_committing(&[VICIOUS_BLOW]);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 2);
}

/// The same Fight without committing Vicious Blow deals the base `1` —
/// confirms the +1 comes from the card, not the action.
#[test]
fn fight_without_vicious_blow_deals_base_damage() {
    let session = fight_committing(&[]);

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 1);
}
