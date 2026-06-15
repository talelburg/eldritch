//! End-to-end commit-time attack buff with a mock `CardRegistry`: a
//! skill card carrying `[Trigger::OnCommit] BoostAttackDamage(1)`
//! (Vicious-Blow-shaped). Verifies that committing it to a Fight skill
//! test adds +1 to the attack's damage on success, and that the
//! `OnCommit` firing path fires committed cards' effects at the commit
//! step (no such firing existed before #307).
//!
//! Lives at `crates/game-core/tests/` (its own integration-test binary,
//! hence its own process + `OnceLock<CardRegistry>`). No real card has an
//! `OnCommit` ability yet — Vicious Blow 01025 (the consumer, #240) is the
//! first; until then this mock skill exercises the full commit path.

use std::sync::OnceLock;

use game_core::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use game_core::dsl::{boost_attack_damage, on_commit, Ability};
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, InvestigatorId, Phase, TokenModifiers,
};
use game_core::test_support::{test_enemy, test_investigator, GameStateBuilder};
use game_core::{assert_event, Action, InputResponse, PlayerAction};

/// Mock skill: combat icon + `[OnCommit] that attack deals +1 damage`.
const SKILL: &str = "VBLOW-MOCK";

fn skill_metadata() -> CardMetadata {
    CardMetadata {
        code: SKILL.to_owned(),
        name: "Mock Vicious Blow".to_owned(),
        traits: vec!["Practiced".to_owned()],
        text: Some(
            "If this skill test is successful during an attack, that attack deals +1 damage."
                .to_owned(),
        ),
        pack_code: "_mock".to_owned(),
        kind: CardKind::Skill {
            class: Class::Guardian,
            xp: Some(0),
            skill_icons: SkillIcons {
                combat: 1,
                ..SkillIcons::default()
            },
            deck_limit: 2,
        },
    }
}

fn skill_metadata_static() -> &'static CardMetadata {
    static M: OnceLock<CardMetadata> = OnceLock::new();
    M.get_or_init(skill_metadata)
}

fn mock_metadata_for(code: &CardCode) -> Option<&'static CardMetadata> {
    (code.as_str() == SKILL).then(skill_metadata_static)
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        SKILL => Some(vec![on_commit(boost_attack_damage(1))]),
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

/// Board: the controller (combat 3) engaged with one enemy (fight 2,
/// health 10 so the dealt damage is observable, not clamped), the mock
/// skill in hand, a `Numeric(0)` chaos bag for a deterministic success.
fn board() -> (game_core::GameState, InvestigatorId, EnemyId) {
    install_mock_registry();
    let id = InvestigatorId(1);
    let enemy_id = EnemyId(100);

    let mut inv = test_investigator(1);
    inv.skills.combat = 3;
    inv.hand = vec![CardCode::new(SKILL)];

    let mut enemy = test_enemy(100, "Ghoul");
    enemy.fight = 2;
    enemy.max_health = 10;
    enemy.engaged_with = Some(id);

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_active_investigator(id)
        .with_turn_order([id])
        .with_investigator(inv)
        .with_enemy(enemy)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, enemy_id)
}

fn fight_action(inv: InvestigatorId, enemy: EnemyId) -> Action {
    Action::Player(PlayerAction::Fight {
        investigator: inv,
        enemy,
    })
}

/// Committing the `OnCommit` skill to a successful Fight test deals
/// `1 base + 1 bonus = 2` damage.
#[test]
fn committing_vicious_blow_adds_one_attack_damage() {
    let (state, id, enemy_id) = board();

    let paused = game_core::engine::apply(state, fight_action(id, enemy_id));
    assert!(matches!(
        paused.outcome,
        EngineOutcome::AwaitingInput { .. }
    ));

    let result = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![0] },
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(result.events, Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(result.state.enemies[&enemy_id].damage, 2);
}

/// Without committing the skill, the same Fight deals the base `1`
/// damage — the `OnCommit` buff only applies when committed (regression
/// guard that the accumulator defaults to 0).
#[test]
fn fight_without_commit_deals_base_damage() {
    let (state, id, enemy_id) = board();

    let paused = game_core::engine::apply(state, fight_action(id, enemy_id));
    let result = game_core::engine::apply(
        paused.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::CommitCards { indices: vec![] },
        }),
    );

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_event!(result.events, Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(result.state.enemies[&enemy_id].damage, 1);
}
