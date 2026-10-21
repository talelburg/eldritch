//! Machete (01020)'s conditional extra damage: the `+1` fires only when the
//! **attacked** enemy is the sole enemy engaged with the actor.
//!
//! A Fight's candidates are every co-located enemy, so "engaged with exactly
//! one enemy" is not the same question as "the enemy I am attacking is that
//! one" — the cases below pin both the bonus and each way of losing it:
//!
//! - sole engaged enemy attacked → `1 + 1 = 2`;
//! - two enemies engaged, one picked → `1 + 0 = 1` (multi-target path);
//! - engaged enemy present but an unengaged co-located enemy attacked → `1`;
//! - engaged with nothing → `1`;
//! - enemy engaged with another investigator → `1` (both FAQ exclusions).
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, Enemy, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const MACHETE: &str = "01020";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Board with Machete in play, `enemy_count` enemies engaged with the actor.
///
/// `combat 4 vs fight 3` with a `Numeric(0)` bag → always succeeds.
fn board(enemy_count: u32) -> GameState {
    board_with(enemy_count, 0).build()
}

/// A fight-3, health-3 Ghoul. Placed at the actor's location by the builder.
fn ghoul(id: u32, name: &str) -> Enemy {
    let mut enemy = test_support::test_enemy(id, name);
    enemy.fight = 3;
    enemy.max_health = 3;
    enemy
}

/// Board with Machete in play, `engaged` enemies engaged with the actor and
/// `unengaged` enemies merely *co-located* with them (a Fight reaches both). Engaged enemies take ids `100+n`, unengaged `200+n`.
fn board_with(engaged: u32, unengaged: u32) -> GameStateBuilder {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Combat, 4)
                .in_play([MACHETE]),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default());

    for n in 0..engaged {
        // Co-located: weapon Fight targets enemies at your location.
        builder = builder.with_enemy_engaged(ghoul(100 + n, "Ghoul"), INV);
    }
    for n in 0..unengaged {
        let mut enemy = ghoul(200 + n, "Aloof Ghoul");
        enemy.current_location = Some(HERE);
        builder = builder.with_enemy(enemy);
    }

    builder
}

/// Activating Machete's Fight ability on `state`.
fn machete_fight(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, MACHETE)),
        address: AbilityAddress::Printed(0),
    }
}

/// Activate Machete against its lone candidate, committing nothing, back to
/// the open-turn menu.
fn activate_machete(state: GameState) -> TestSession {
    let fight = machete_fight(&state);
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&fight)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// With exactly one enemy engaged, a successful Machete Fight deals
/// `1 (base) + 1 (sole-engaged) = 2` damage.
#[test]
fn sole_engaged_enemy_gets_bonus_damage() {
    let session = activate_machete(board(1));
    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(session.state().enemies[&EnemyId(100)].damage, 2);
}

/// Activate Machete on a board with 2+ candidates: the session rests at the
/// target pick.
fn to_target_pick(state: GameState) -> TestSession {
    let fight = machete_fight(&state);
    TestSession::new(state, TEST_REGISTRIES).take(&fight)
}

/// [`to_target_pick`], attack `enemy`, then commit nothing back to the
/// open-turn menu.
fn activate_and_attack(state: GameState, enemy: EnemyId) -> TestSession {
    let session = to_target_pick(state)
        .pick(OptionTarget::Enemy(enemy))
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// With two enemies engaged, activating Machete suspends for a target pick.
/// After picking enemy 100, the Fight resolves against it for `1 + 0 = 1`
/// damage — the attacked enemy is engaged, but it is not the *only* enemy
/// engaged with the actor. Enemy 101 is untouched.
#[test]
fn two_enemies_engaged_suspends_for_pick_then_attacks_chosen() {
    // Step 1: activate → should suspend for enemy target pick (NOT rejected).
    let session = to_target_pick(board(2));
    assert_eq!(
        session.offered_at(&OptionTarget::Enemy(EnemyId(101))),
        1,
        "expected the target pick; got {:?}",
        session.prompt()
    );

    // Step 2: pick enemy 100, then answer the commit window (no commits): play
    // comes back to the open-turn menu.
    let session = session.pick(OptionTarget::Enemy(EnemyId(100))).commit(&[]);
    session.assert_at_turn_menu(INV);

    // Enemy 100 (chosen) took 1 damage; enemy 101 (not chosen) took 0.
    assert_event!(
        session.events(),
        Event::EnemyDamaged {
            enemy: EnemyId(100),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        session.state().enemies[&EnemyId(100)].damage,
        1,
        "chosen enemy took 1 damage"
    );
    assert_eq!(
        session.state().enemies[&EnemyId(101)].damage,
        0,
        "unchosen enemy untouched"
    );
}

/// Regression for #592: engaged with exactly one enemy, attacking a *different*, unengaged
/// co-located enemy gets **no** bonus — the attacked enemy is not "the only
/// enemy engaged with you", it is not engaged with you at all.
///
/// `ArkhamDB` FAQ (01020): "Machete will not provide a damage bonus for attacking
/// a disengaged enemy (evaded or *Aloof*), or an enemy engaged to another
/// player. You can spend an action to Engage an enemy to gain the damage bonus."
#[test]
fn unengaged_co_located_target_gets_no_bonus() {
    // Enemy 100 engaged, enemy 200 co-located but unengaged; attack 200.
    let session = activate_and_attack(board_with(1, 1).build(), EnemyId(200));

    assert_event!(
        session.events(),
        Event::EnemyDamaged {
            enemy: EnemyId(200),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        session.state().enemies[&EnemyId(200)].damage,
        1,
        "attacked enemy is unengaged → base damage only, no sole-engaged bonus"
    );
    assert_eq!(
        session.state().enemies[&EnemyId(100)].damage,
        0,
        "the engaged enemy was not the target"
    );
}

/// With *no* enemy engaged with you at all, attacking a co-located enemy
/// gets no bonus either — "the only enemy engaged with you" is vacuously false
/// when you are engaged with nothing.
///
/// Unlike its two siblings this board does *not* discriminate against an
/// `EngagedEnemies == 1` encoding, which reads 0 here and also withholds the
/// bonus. It guards a different wrong answer: an encoding phrased as "no
/// engaged enemy *other than* the target" would count 0 and grant the bonus.
#[test]
fn no_engaged_enemies_at_all_gets_no_bonus() {
    // Single co-located unengaged enemy → target auto-binds, no pick needed.
    let session = activate_machete(board_with(0, 1).build());

    assert_event!(
        session.events(),
        Event::EnemyDamaged {
            enemy: EnemyId(200),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        session.state().enemies[&EnemyId(200)].damage,
        1,
        "engaged with nothing → base damage only"
    );
}

/// Regression for #592: an enemy engaged with *another investigator* is not "engaged with
/// you", so it gets no bonus — the second exclusion in the 01020 FAQ ("or an
/// enemy engaged to another player").
///
/// The actor is deliberately engaged with exactly one enemy of their own
/// (enemy 100), so a count-only encoding would read `EngagedEnemies == 1` and
/// grant the bonus: this board discriminates against that encoding rather than
/// merely agreeing with it.
#[test]
fn enemy_engaged_with_another_investigator_gets_no_bonus() {
    const OTHER: InvestigatorId = InvestigatorId(2);

    // A second investigator at the same location, engaged with enemy 200. The
    // actor remains engaged with enemy 100 only.
    let state = board_with(1, 0)
        .with_seat(seat(2).at(HERE))
        .with_enemy_engaged(ghoul(200, "Aloof Ghoul"), OTHER)
        .build();

    // Attack enemy 200.
    let session = activate_and_attack(state, EnemyId(200));

    assert_event!(
        session.events(),
        Event::EnemyDamaged {
            enemy: EnemyId(200),
            amount: 1,
            ..
        }
    );
    assert_eq!(
        session.state().enemies[&EnemyId(200)].damage,
        1,
        "enemy engaged to another player → base damage only"
    );
    assert_eq!(
        session.state().enemies[&EnemyId(100)].damage,
        0,
        "the actor's own engaged enemy was not the target"
    );
}

/// With no enemy at your location, the activation is rejected before any cost
/// is paid — by the handler itself, which a client can reach without
/// consulting the menu (the menu does not offer it).
#[test]
fn no_co_located_enemy_activation_is_rejected_precost() {
    let state = board(0);
    let actions_before = state.investigators[&INV].actions_remaining;
    let fight = machete_fight(&state);
    let session = TestSession::new(state, TEST_REGISTRIES).submit_off_menu(&fight);
    session.expect_rejected();
    // No cost paid: actions unchanged.
    assert_eq!(
        session.state().investigators[&INV].actions_remaining,
        actions_before
    );
}
