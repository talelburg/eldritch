//! End-to-end weapon flow with a mock `CardRegistry`: a firearm-shaped
//! asset that carries `Uses (4 ammo)` and an `[action] Spend 1 ammo:
//! Fight` activated ability whose effect is `Effect::Fight`.
//!
//! Lives at `crates/game-core/tests/` (its own integration-test binary,
//! hence its own process + `OnceLock<CardRegistry>`) so the mock
//! registry doesn't collide with the in-crate tests. No real card has a
//! weapon ability yet — Roland's .38 Special (C5c) is the first; until
//! then a mock card exercises the full path.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons, Slot, UseKind, Uses};
use game_core::dsl::{activated, fight, Ability, Cost, IntExpr};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, Phase,
    TokenModifiers,
};
use game_core::test_support::{apply_no_commits, test_enemy, test_investigator, GameStateBuilder};
use game_core::{assert_event, Action, PlayerAction};

/// Mock firearm: `Uses (4 ammo)`, `[action] Spend 1 ammo: Fight. +1
/// [combat], +1 damage.`
const WEAPON: &str = "WEAP1";

fn weapon_metadata() -> CardMetadata {
    CardMetadata {
        code: WEAPON.to_owned(),
        name: "Mock Firearm".to_owned(),
        traits: vec!["Item".to_owned(), "Weapon".to_owned(), "Firearm".to_owned()],
        text: Some("Uses (4 ammo).\n[action] Spend 1 ammo: Fight.".to_owned()),
        pack_code: "_mock".to_owned(),
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
            fight(IntExpr::Lit(1), 1),
        )]),
        _ => None,
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(game_core::card_registry::CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
            native_effect_for: |_| None,
        });
    });
}

/// Board: the weapon in play (instance 0) with a freshly-seeded 4-ammo
/// pool, the controller active with combat 3 in the Investigation phase,
/// engaged with `enemy_count` enemies (fight 3, health 3). A `Numeric(0)`
/// chaos bag makes the combat total deterministic.
fn board_with_weapon(enemy_count: u32) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    install_mock_registry();
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
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default());
    for n in 0..enemy_count {
        let mut enemy = test_enemy(100 + n, "Ghoul");
        enemy.fight = 3;
        enemy.max_health = 3;
        enemy.engaged_with = Some(id);
        builder = builder.with_enemy(enemy);
    }
    let state = builder.with_investigator(inv).build();
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
    install_mock_registry();
    let id = InvestigatorId(1);
    let mut inv = test_investigator(1);
    inv.hand.push(CardCode::new(WEAPON));
    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_investigator(inv)
        .build();

    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::PlayCard {
            investigator: id,
            hand_index: 0,
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
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
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id: weapon,
            ability_index: 0,
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
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
fn weapon_fight_rejects_when_no_enemy_engaged() {
    // 0 engaged → illegal (no target); reject before charging anything.
    let (state, id, weapon) = board_with_weapon(0);
    let actions_before = state.investigators[&id].actions_remaining;
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id: weapon,
            ability_index: 0,
        }),
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
fn weapon_fight_rejects_when_two_enemies_engaged() {
    // 2+ engaged → deferred multi-target selection; reject, nothing charged.
    let (state, id, weapon) = board_with_weapon(2);
    let actions_before = state.investigators[&id].actions_remaining;
    let result = apply_no_commits(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id: weapon,
            ability_index: 0,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_eq!(ammo_remaining(&result.state, id, weapon), 4);
    assert_eq!(
        result.state.investigators[&id].actions_remaining,
        actions_before
    );
}
