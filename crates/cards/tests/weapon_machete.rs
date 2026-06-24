//! #300 behaviour test: Machete (01020) conditional extra damage.
//!
//! Verifies that a successful Machete Fight deals `1 + 1 = 2` damage when the
//! attacked enemy is the sole enemy engaged with the actor ("sole-engaged" bonus
//! active), and that with two enemies engaged the activation is rejected by the
//! engine's current pre-#401 gate (which defers multi-target selection).
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, InvestigatorId,
    LocationId, Phase, TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, test_enemy, test_investigator, test_location, GameStateBuilder,
};
use game_core::{assert_event, Action, PlayerAction};

const MACHETE: &str = "01020";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const MACHETE_INST: CardInstanceId = CardInstanceId(0);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Board with Machete in play, `enemy_count` enemies engaged with the actor.
///
/// `combat 4 vs fight 3` with a `Numeric(0)` bag → always succeeds.
fn board(enemy_count: u32) -> game_core::GameState {
    install();
    let mut inv = test_investigator(1);
    inv.skills.combat = 4;
    let machete = CardInPlay::enter_play(CardCode::new(MACHETE), MACHETE_INST);
    inv.cards_in_play.push(machete);

    let location = test_location(10, "Study");

    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(location)
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default());

    for n in 0..enemy_count {
        let mut enemy = test_enemy(100 + n, "Ghoul");
        enemy.fight = 3;
        enemy.max_health = 3;
        enemy.engaged_with = Some(INV);
        builder = builder.with_enemy(enemy);
    }

    builder.build()
}

fn activate_machete(state: game_core::GameState) -> game_core::engine::ApplyResult {
    apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: INV,
            instance_id: MACHETE_INST,
            ability_index: 0,
        }),
    )
}

/// With exactly one enemy engaged, a successful Machete Fight deals
/// `1 (base) + 1 (sole-engaged) = 2` damage.
#[test]
fn sole_engaged_enemy_gets_bonus_damage() {
    let r = activate_machete(board(1));
    assert_eq!(r.outcome, EngineOutcome::Done);
    assert_event!(r.events, Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(r.state.enemies[&EnemyId(100)].damage, 2);
}

/// With two enemies engaged the activation is rejected by the pre-#401
/// multi-target gate — nothing is charged and no damage is dealt.
///
/// Once multi-target Fight (#401) lands this test should be replaced with
/// one that verifies the attack deals `1 + 0 = 1` damage (`extra_damage` is
/// 0 because `EngagedEnemies` == 2, not 1).
#[test]
fn two_enemies_engaged_activation_is_rejected() {
    let state = board(2);
    let actions_before = state.investigators[&INV].actions_remaining;
    let r = activate_machete(state);
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "expected Rejected; got {:?}",
        r.outcome
    );
    // No cost paid: actions unchanged.
    assert_eq!(
        r.state.investigators[&INV].actions_remaining,
        actions_before
    );
}
