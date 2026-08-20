//! #300 / #592 behaviour test: Machete (01020) conditional extra damage.
//!
//! The `+1` fires only when the **attacked** enemy is the sole enemy engaged
//! with the actor. Since #451 widened `Effect::Fight`'s candidates from
//! engaged-only to every co-located enemy, "engaged with exactly one enemy" is
//! not the same question as "the enemy I am attacking is that one" — the cases
//! below pin both the bonus and each way of losing it:
//!
//! - sole engaged enemy attacked → `1 + 1 = 2`;
//! - two enemies engaged, one picked → `1 + 0 = 1` (multi-target path, #449);
//! - engaged enemy present but an unengaged co-located enemy attacked → `1`;
//! - engaged with nothing → `1`;
//! - enemy engaged with another investigator → `1` (both FAQ exclusions).
//!
//! Own process → installs `cards::REGISTRY`.

use game_core::engine::EngineOutcome;
use game_core::engine::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, EnemyId,
    InvestigatorId, LocationId, Phase, TokenModifiers,
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

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Board with Machete in play, `enemy_count` enemies engaged with the actor.
///
/// `combat 4 vs fight 3` with a `Numeric(0)` bag → always succeeds.
fn board(enemy_count: u32) -> game_core::GameState {
    board_with(enemy_count, 0)
}

/// Board with Machete in play, `engaged` enemies engaged with the actor and
/// `unengaged` enemies merely *co-located* with them (the scope #451 widened
/// `Effect::Fight` to). Engaged enemies take ids `100+n`, unengaged `200+n`, so
/// the `BTreeMap`-ascending `OptionId` order is engaged-first.
fn board_with(engaged: u32, unengaged: u32) -> game_core::GameState {
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

    for n in 0..engaged {
        let mut enemy = test_enemy(100 + n, "Ghoul");
        enemy.fight = 3;
        enemy.max_health = 3;
        enemy.engaged_with = Some(INV);
        enemy.current_location = Some(LOC); // co-located: weapon Fight targets enemies at your location
        builder = builder.with_enemy(enemy);
    }
    for n in 0..unengaged {
        let mut enemy = test_enemy(200 + n, "Aloof Ghoul");
        enemy.fight = 3;
        enemy.max_health = 3;
        enemy.engaged_with = None;
        enemy.current_location = Some(LOC);
        builder = builder.with_enemy(enemy);
    }

    builder.build()
}

fn activate_machete(state: game_core::GameState) -> game_core::engine::ApplyResult {
    TestSession::new(state)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(MACHETE_INST),
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
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    assert_event!(r.events, Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(r.state.enemies[&EnemyId(100)].damage, 2);
}

/// With two enemies engaged, activating Machete suspends for a target pick.
/// After picking enemy 100 (OptionId(0)), the Fight resolves against it for
/// `1 + 0 = 1` damage — the attacked enemy is engaged, but it is not the *only*
/// enemy engaged with the actor. Enemy 101 is untouched.
#[test]
fn two_enemies_engaged_suspends_for_pick_then_attacks_chosen() {
    let state = board(2);

    // Step 1: activate → should suspend for enemy target pick (NOT rejected).
    let r1 = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(MACHETE_INST),
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
    assert!(
        matches!(r2.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected the open-turn menu after pick + commit; got {:?}",
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

/// Activate Machete on a board with 2+ candidates: suspend for the target pick,
/// answer it with `option`, then drain the commit window to the open-turn menu.
fn activate_and_pick(state: game_core::GameState, option: u32) -> game_core::engine::ApplyResult {
    let r1 = take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(MACHETE_INST),
            ability_index: 0,
        },
    );
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected AwaitingInput for target pick; got {:?}",
        r1.outcome
    );
    apply_no_commits(
        r1.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(option)),
        }),
    )
}

/// #592: engaged with exactly one enemy, attacking a *different*, unengaged
/// co-located enemy gets **no** bonus — the attacked enemy is not "the only
/// enemy engaged with you", it is not engaged with you at all.
///
/// `ArkhamDB` FAQ (01020): "Machete will not provide a damage bonus for attacking
/// a disengaged enemy (evaded or *Aloof*), or an enemy engaged to another
/// player. You can spend an action to Engage an enemy to gain the damage bonus."
#[test]
fn unengaged_co_located_target_gets_no_bonus() {
    // Enemy 100 engaged, enemy 200 co-located but unengaged; pick 200 (OptionId(1)).
    let r = activate_and_pick(board_with(1, 1), 1);

    assert_event!(
        r.events,
        Event::EnemyDamaged {
            enemy: EnemyId(200),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        r.state.enemies[&EnemyId(200)].damage,
        1,
        "attacked enemy is unengaged → base damage only, no sole-engaged bonus"
    );
    assert_eq!(
        r.state.enemies[&EnemyId(100)].damage,
        0,
        "the engaged enemy was not the target"
    );
}

/// #592: with *no* enemy engaged with you at all, attacking a co-located enemy
/// gets no bonus either — "the only enemy engaged with you" is vacuously false
/// when you are engaged with nothing.
///
/// Unlike its two siblings this board does *not* discriminate against the #592
/// bug (the old `EngagedEnemies == 1` encoding read 0 here and also withheld the
/// bonus). It guards a different wrong answer: an encoding phrased as "no
/// engaged enemy *other than* the target" would count 0 and grant the bonus.
#[test]
fn no_engaged_enemies_at_all_gets_no_bonus() {
    // Single co-located unengaged enemy → target auto-binds, no pick needed.
    let r = activate_machete(board_with(0, 1));

    assert_event!(
        r.events,
        Event::EnemyDamaged {
            enemy: EnemyId(200),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        r.state.enemies[&EnemyId(200)].damage,
        1,
        "engaged with nothing → base damage only"
    );
}

/// #592: an enemy engaged with *another investigator* is not "engaged with
/// you", so it gets no bonus — the second exclusion in the 01020 FAQ ("or an
/// enemy engaged to another player").
///
/// The actor is deliberately engaged with exactly one enemy of their own
/// (enemy 100), so the old count-only encoding would read `EngagedEnemies == 1`
/// and grant the bonus: this board discriminates against the #592 bug rather
/// than merely agreeing with it.
#[test]
fn enemy_engaged_with_another_investigator_gets_no_bonus() {
    const OTHER: InvestigatorId = InvestigatorId(2);

    let mut state = board_with(1, 1);
    // Seat a second investigator at the same location and engage enemy 200
    // with them. The actor remains engaged with enemy 100 only.
    let mut other = test_investigator(2);
    other.current_location = Some(LOC);
    state.investigators.insert(OTHER, other);
    state.enemies.get_mut(&EnemyId(200)).unwrap().engaged_with = Some(OTHER);

    // Attack enemy 200 (OptionId(1) — co-located candidates in EnemyId order).
    let r = activate_and_pick(state, 1);

    assert_event!(
        r.events,
        Event::EnemyDamaged {
            enemy: EnemyId(200),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        r.state.enemies[&EnemyId(200)].damage,
        1,
        "enemy engaged to another player → base damage only"
    );
    assert_eq!(
        r.state.enemies[&EnemyId(100)].damage,
        0,
        "the actor's own engaged enemy was not the target"
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
            source: AbilitySource::InPlay(MACHETE_INST),
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
