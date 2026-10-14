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

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId, LocationId, TokenModifiers,
};
use game_core::test_support::{self, seat, MockRegistry, TestSession};

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
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind: CardKind::Skill {
            class: Class::Guardian,
            xp: Some(0),
            skill_icons: SkillIcons {
                combat: 1,
                ..SkillIcons::default()
            },
            deck_limit: 2,
            commit_limit: None,
        },
    }
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(skill_metadata())
            .with_abilities(SKILL, || vec![dsl::on_commit(dsl::boost_attack_damage(1))])
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
const GHOUL: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(10);

/// Board: the controller (combat 3) engaged with one enemy (fight 2,
/// health 10 so the dealt damage is observable, not clamped), the mock
/// skill in hand, a `Numeric(0)` chaos bag for a deterministic success.
fn board() -> GameStateBuilder {
    let mut enemy = test_support::test_enemy(GHOUL.0, "Ghoul");
    enemy.fight = 2;
    enemy.max_health = 10;

    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE).hand([SKILL]))
        .with_location(test_support::test_location(HERE.0, "Study"))
        // Co-located: Fight is location-gated (#401).
        .with_enemy_engaged(enemy, INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
}

/// Fight the Ghoul, committing `commit` to the test. The attack resolves back
/// to the turn menu.
fn fight_committing(commit: &[&str]) -> TestSession {
    let session = board()
        .session()
        .take(&TurnAction::Fight {
            investigator: INV,
            enemy: GHOUL,
        })
        .commit(commit);
    session.assert_at_turn_menu(INV);
    session
}

/// Committing the `OnCommit` skill to a successful Fight test deals
/// `1 base + 1 bonus = 2` damage.
#[test]
fn committing_vicious_blow_adds_one_attack_damage() {
    let session = fight_committing(&[SKILL]);

    assert_event!(session.events(), Event::EnemyDamaged { amount: 2, .. });
    assert_eq!(session.state().enemies[&GHOUL].damage, 2);
}

/// Without committing the skill, the same Fight deals the base `1`
/// damage — the `OnCommit` buff only applies when committed (regression
/// guard that the accumulator defaults to 0).
#[test]
fn fight_without_commit_deals_base_damage() {
    let session = fight_committing(&[]);

    assert_event!(session.events(), Event::EnemyDamaged { amount: 1, .. });
    assert_eq!(session.state().enemies[&GHOUL].damage, 1);
}
