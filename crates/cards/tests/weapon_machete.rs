//! #300 behaviour test: Machete (01020) conditional extra damage.
//!
//! Verifies that a successful Machete Fight deals `1 + 1 = 2` damage when the
//! attacked enemy is the sole enemy engaged with the actor ("sole-engaged" bonus
//! active), and covers the multi-target activation path (#449): with 2 enemies
//! engaged the activation suspends for a target pick (`AwaitingInput`), and after
//! resuming with the first enemy chosen the Fight resolves against that enemy
//! for `1 + 0 = 1` damage (`extra_damage` is 0 because `EngagedEnemies` == 2).
//!
//! Own process → installs `cards::REGISTRY`.

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::engine::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId, InvestigatorId,
    LocationId, Phase, TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, dispatch_turn_action_unchecked, take_turn_action, test_enemy,
    test_investigator, test_location, GameStateBuilder, TestSession,
};
use game_core::{assert_event, Action, InputResponse, OptionId, PlayerAction};

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
        .with_investigator_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default());

    for n in 0..enemy_count {
        let mut enemy = test_enemy(100 + n, "Ghoul");
        enemy.fight = 3;
        enemy.max_health = 3;
        enemy.engaged_with = Some(INV);
        enemy.current_location = Some(LOC); // co-located: weapon Fight targets enemies at your location
        builder = builder.with_enemy(enemy);
    }

    builder.build()
}

fn activate_machete(state: game_core::GameState) -> game_core::engine::ApplyResult {
    TestSession::new(state)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            instance_id: MACHETE_INST,
            ability_index: 0,
        })
        .resolve_choices(|c| {
            c.commit_cards(&[]);
        })
        .run()
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

/// With two enemies engaged, activating Machete suspends for a target pick.
/// After picking enemy 100 (OptionId(0)), the Fight resolves against it for
/// `1 + 0 = 1` damage (`extra_damage` is 0 because `EngagedEnemies` == 2).
/// Enemy 101 is untouched.
#[test]
fn two_enemies_engaged_suspends_for_pick_then_attacks_chosen() {
    let state = board(2);

    // Step 1: activate → should suspend for enemy target pick (NOT rejected).
    let r1 = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            instance_id: MACHETE_INST,
            ability_index: 0,
        },
    );
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected AwaitingInput for target pick; got {:?}",
        r1.outcome
    );

    // Step 2: pick enemy 100 (OptionId(0) — enemies in BTreeMap ascending order).
    // Then drain the commit window (no commits) to Done.
    let r2 = apply_no_commits(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(0)),
        }),
    );
    assert_eq!(
        r2.outcome,
        EngineOutcome::Done,
        "expected Done after pick + commit; got {:?}",
        r2.outcome
    );

    // Enemy 100 (chosen) took 1 damage; enemy 101 (not chosen) took 0.
    assert_event!(
        r2.events,
        Event::EnemyDamaged {
            enemy: EnemyId(100),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        r2.state.enemies[&EnemyId(100)].damage,
        1,
        "chosen enemy took 1 damage"
    );
    assert_eq!(
        r2.state.enemies[&EnemyId(101)].damage,
        0,
        "unchosen enemy untouched"
    );
}

/// With no enemy at your location, the activation is rejected before any cost
/// is paid.
#[test]
fn no_co_located_enemy_activation_is_rejected_precost() {
    let state = board(0);
    let actions_before = state.investigators[&INV].actions_remaining;
    let r = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            instance_id: MACHETE_INST,
            ability_index: 0,
        },
    );
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
