//! C5e (#240) integration: Vicious Blow 01025 end-to-end against the real
//! `cards::REGISTRY`. The Guardian L0 skill commits to a Fight test and,
//! on success, adds +1 to the attack's damage via its `OnCommit`
//! `BoostAttackDamage(1)` ability (engine machinery from #307 / PR #308).
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, InvestigatorId, Phase, TokenModifiers,
};
use game_core::test_support::{
    drive, test_enemy, test_investigator, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event, Action, EngineOutcome, PlayerAction};

const VICIOUS_BLOW: &str = "01025";
const INV: InvestigatorId = InvestigatorId(1);
const ENEMY: EnemyId = EnemyId(100);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Board: the controller (combat 3) engaged with one enemy (fight 2,
/// health 10 so the dealt damage is observable, not clamped), Vicious
/// Blow in hand, a `Numeric(0)` chaos bag for a deterministic success.
fn board() -> GameState {
    let mut inv = test_investigator(1);
    inv.skills.combat = 3;
    inv.hand = vec![CardCode::new(VICIOUS_BLOW)];

    let mut enemy = test_enemy(100, "Ghoul");
    enemy.fight = 2;
    enemy.max_health = 10;
    enemy.engaged_with = Some(INV);

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build()
}

use game_core::state::GameState;

fn fight_action() -> Action {
    Action::Player(PlayerAction::Fight {
        investigator: INV,
        enemy: ENEMY,
    })
}

/// Committing Vicious Blow to a successful Fight deals `1 base + 1 = 2`.
#[test]
fn committing_vicious_blow_adds_one_attack_damage() {
    install();
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[CardCode::new(VICIOUS_BLOW)]);

    let r = drive(board(), fight_action(), resolver);

    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(r.state.enemies[&ENEMY].damage, 2);
}

/// The same Fight without committing Vicious Blow deals the base `1` —
/// confirms the +1 comes from the card, not the action.
#[test]
fn fight_without_vicious_blow_deals_base_damage() {
    install();
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);

    let r = drive(board(), fight_action(), resolver);

    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(r.state.enemies[&ENEMY].damage, 1);
}
