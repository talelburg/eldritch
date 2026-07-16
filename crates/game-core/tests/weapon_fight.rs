//! End-to-end weapon flow with a mock `CardRegistry`: a firearm-shaped
//! asset that carries `Uses (4 ammo)` and an `[action] Spend 1 ammo:
//! Fight` activated ability whose effect is `Effect::Fight`.
//!
//! Lives at `crates/game-core/tests/` (its own integration-test binary,
//! hence its own process + `OnceLock<CardRegistry>`) so the mock
//! registry doesn't collide with the in-crate tests. No real card has a
//! weapon ability yet — Roland's .38 Special (C5c) is the first; until
//! then a mock card exercises the full path.

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot, UseKind, Uses};
use game_core::dsl::{activated, fight, Ability, Cost, IntExpr};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, LocationId, Phase,
    TokenModifiers,
};
use game_core::test_support::{
    apply_no_commits, dispatch_turn_action_unchecked, test_enemy, test_investigator, test_location,
    GameStateBuilder,
};
use game_core::{apply, assert_event, Action, InputResponse, OptionId, PlayerAction, TurnAction};
use std::sync::OnceLock;

/// Mock firearm: `Uses (4 ammo)`, `[action] Spend 1 ammo: Fight. +1
/// [combat], +1 damage.`
const WEAPON: &str = "WEAP1";

fn weapon_metadata() -> CardMetadata {
    CardMetadata {
        code: WEAPON.to_owned(),
        name: "Mock Firearm".to_owned(),
        traits: vec!["Item".to_owned(), "Weapon".to_owned(), "Firearm".to_owned()],
        text: Some("Uses (4 ammo).\n[action] Spend 1 ammo: Fight.".to_owned()),
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind: CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![Slot::Hand],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 1,
            uses: Some(Uses {
                kind: UseKind::Ammo,
                count: 4,
                discard_when_empty: false,
            }),
            play_only_during_turn: false,
        },
    }
}

fn weapon_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(weapon_metadata)
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    (code.as_str() == WEAPON).then(weapon_metadata_static)
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        // [action] Spend 1 ammo: Fight. +1 [combat], +1 damage.
        WEAPON => Some(vec![activated(
            1,
            vec![Cost::SpendUses {
                kind: UseKind::Ammo,
                count: 1,
            }],
            fight(IntExpr::Lit(1), 1u8),
        )]),
        _ => None,
    }
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    let _ = game_core::card_registry::install(game_core::card_registry::CardRegistry {
        metadata_for: mock_metadata_for,
        abilities_for: mock_abilities_for,
        native_effect_for: |_| None,
        native_eligibility_for: |_| None,
    });
}

/// The single location the controller and its enemies share. Co-location is
/// load-bearing: a weapon Fight targets any enemy *at your location*
/// (`EntityScope::At(Here)`), so the board must place everyone there.
const LOC: LocationId = LocationId(1);

/// Board: the weapon in play (instance 0) with a freshly-seeded 4-ammo
/// pool, the controller active with combat 3 in the Investigation phase,
/// at [`LOC`] and engaged with `enemy_count` enemies (fight 3, health 3) that
/// are co-located there. A `Numeric(0)` chaos bag makes the combat total
/// deterministic.
fn board_with_weapon(enemy_count: u32) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    board_with_enemies(enemy_count, true)
}

/// As [`board_with_weapon`], but `engaged` controls whether the co-located
/// enemies are engaged with the controller. Unengaged-but-co-located is a
/// legal weapon Fight target (an Aloof enemy, or one engaged with another
/// investigator in MP) — #451.
fn board_with_enemies(
    enemy_count: u32,
    engaged: bool,
) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    let id = InvestigatorId(1);
    let weapon_inst = CardInstanceId(0);

    let mut inv = test_investigator(1);
    inv.skills.combat = 3;
    let mut weapon = CardInPlay::enter_play(CardCode::new(WEAPON), weapon_inst);
    weapon.uses.insert(UseKind::Ammo, 4); // seeded as play_card would
    inv.cards_in_play.push(weapon);

    let mut builder = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_location(test_location(1, "Study"))
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default());
    for n in 0..enemy_count {
        let mut enemy = test_enemy(100 + n, "Ghoul");
        enemy.fight = 3;
        enemy.max_health = 3;
        enemy.current_location = Some(LOC);
        enemy.engaged_with = engaged.then_some(id);
        builder = builder.with_enemy(enemy);
    }
    let state = builder.with_investigator_at(inv, LOC).build();
    (state, id, weapon_inst)
}

fn ammo_remaining(state: &game_core::GameState, inv: InvestigatorId, weapon: CardInstanceId) -> u8 {
    state.investigators[&inv]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == weapon)
        .and_then(|c| c.uses.get(&UseKind::Ammo).copied())
        .expect("weapon must carry an ammo pool")
}

#[test]
fn play_card_seeds_the_ammo_pool_from_metadata() {
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.hand.push(CardCode::new(WEAPON));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator_turn(id)
        .with_investigator(inv)
        .build();

    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::PlayCard {
                investigator: id,
                hand_index: 0,
            }
        })
        .expect("PlayCard must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );
    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    let weapon = result.state.investigators[&id]
        .cards_in_play
        .first()
        .expect("weapon entered play");
    assert_eq!(weapon.uses.get(&UseKind::Ammo).copied(), Some(4));
}

#[test]
fn weapon_fight_spends_ammo_and_deals_bonus_damage() {
    let (state, id, weapon) = board_with_weapon(1);
    assert_eq!(ammo_remaining(&state, id, weapon), 4);

    // Activate → pays 1 ammo up front, then the Combat test resolves
    // (combat 3 + modifier 1 vs fight 3 → success) dealing 1 + 1 = 2.
    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::ActivateAbility {
                investigator: id,
                instance_id: weapon,
                ability_index: 0,
            }
        })
        .expect("ability must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(
        result.events,
        Event::UsesSpent {
            kind: UseKind::Ammo,
            amount: 1,
            ..
        }
    );
    assert_event!(result.events, Event::SkillTestStarted { difficulty: 3, .. });
    assert_event!(result.events, Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(ammo_remaining(&result.state, id, weapon), 3);
    assert_eq!(
        result.state.enemies[&game_core::state::EnemyId(100)].damage,
        2
    );
}

#[test]
fn weapon_fight_targets_a_co_located_unengaged_enemy() {
    // A co-located but unengaged enemy (e.g. Aloof, or engaged with another
    // investigator in MP) is a legal weapon Fight target — RR: you choose an
    // enemy *at your location* to attack; you need not already be engaged
    // (#451). With exactly one such enemy the target auto-binds (no suspend),
    // mirroring the single-engaged case.
    let (state, id, weapon) = board_with_enemies(1, false);
    assert_eq!(
        state.enemies[&game_core::state::EnemyId(100)].engaged_with,
        None,
        "precondition: the enemy is co-located but NOT engaged"
    );

    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::ActivateAbility {
                investigator: id,
                instance_id: weapon,
                ability_index: 0,
            }
        })
        .expect("ability must be legal");
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );

    assert!(matches!(
        result.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));
    assert_event!(result.events, Event::SkillTestStarted { difficulty: 3, .. });
    assert_event!(
        result.events,
        Event::EnemyDamaged {
            enemy: game_core::state::EnemyId(100),
            amount: 2,
            ..
        }
    );
    assert_eq!(ammo_remaining(&result.state, id, weapon), 3);
    assert_eq!(
        result.state.enemies[&game_core::state::EnemyId(100)].damage,
        2
    );
}

#[test]
fn weapon_fight_rejects_an_enemy_at_a_different_location() {
    // The scope is co-located (`At(Here)`), NOT global: an enemy elsewhere is
    // no target, even though it exists. Guards against widening past the basic
    // Fight action (#451).
    let id = InvestigatorId(1);
    let weapon_inst = CardInstanceId(0);
    let other = LocationId(2);

    let mut inv = test_investigator(1);
    inv.skills.combat = 3;
    let mut weapon = CardInPlay::enter_play(CardCode::new(WEAPON), weapon_inst);
    weapon.uses.insert(UseKind::Ammo, 4);
    inv.cards_in_play.push(weapon);

    let mut enemy = test_enemy(100, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 3;
    enemy.current_location = Some(other); // elsewhere, not the controller's LOC
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_location(test_location(1, "Study"))
        .with_location(test_location(2, "Hallway"))
        .with_enemy(enemy)
        .with_investigator_at(inv, LOC)
        .build();

    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            instance_id: weapon_inst,
            ability_index: 0,
        },
    );
    let EngineOutcome::Rejected { reason } = &result.outcome else {
        panic!("expected Rejected; got {:?}", result.outcome);
    };
    assert!(
        reason.contains("at your location"),
        "should reject for the missing co-located target, not an unrelated \
         precondition; got: {reason}"
    );
    assert_eq!(ammo_remaining(&result.state, id, weapon_inst), 4);
    assert_eq!(
        result.state.enemies[&game_core::state::EnemyId(100)].damage,
        0
    );
}

#[test]
fn weapon_fight_rejects_when_no_co_located_enemy() {
    // No enemy at your location → illegal (no target); reject before charging
    // anything.
    let (state, id, weapon) = board_with_weapon(0);
    let actions_before = state.investigators[&id].actions_remaining;
    let result = dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            instance_id: weapon,
            ability_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // Nothing charged: ammo still 4, actions unchanged.
    assert_eq!(ammo_remaining(&result.state, id, weapon), 4);
    assert_eq!(
        result.state.investigators[&id].actions_remaining,
        actions_before
    );
}

#[test]
fn weapon_fight_with_two_enemies_suspends_for_pick_then_attacks_chosen() {
    // 2+ engaged → suspends for target pick; after picking enemy 100
    // (OptionId(0)), the Fight resolves for 1 + 1 = 2 damage (combat 3 + mod 1
    // vs fight 3 → success; extra_damage == 1 because the weapon's `fight`
    // hardcodes `1u8` as `extra_damage`, not "sole-engaged" conditional).
    let (state, id, weapon) = board_with_weapon(2);

    // Step 1: activation suspends for the target pick.
    let idx = game_core::engine::enumerate::legal_actions(&state)
        .iter()
        .position(|a| {
            a == &TurnAction::ActivateAbility {
                investigator: id,
                instance_id: weapon,
                ability_index: 0,
            }
        })
        .expect("ability must be legal");
    let r1 = apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(u32::try_from(idx).unwrap())),
        }),
    );
    assert!(
        matches!(r1.outcome, EngineOutcome::AwaitingInput { .. }),
        "expected AwaitingInput for target pick; got {:?}",
        r1.outcome
    );

    // Step 2: pick enemy 100 (OptionId(0)), then commit nothing to resolve the test.
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
    // Enemy 100 was attacked (chosen); enemy 101 was not.
    assert_event!(
        r2.events,
        Event::EnemyDamaged {
            enemy: game_core::state::EnemyId(100),
            amount: 2,
            ..
        }
    );
    assert_eq!(r2.state.enemies[&game_core::state::EnemyId(100)].damage, 2);
    assert_eq!(r2.state.enemies[&game_core::state::EnemyId(101)].damage, 0);
    // Ammo was spent on activation.
    assert_eq!(ammo_remaining(&r2.state, id, weapon), 3);
}
