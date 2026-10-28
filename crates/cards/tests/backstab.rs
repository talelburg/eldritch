//! Backstab 01051 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01051):
//!
//! > <b>Fight.</b> This attack uses [agility] instead of [combat]. This attack
//! > deals +2 damage.
//!
//! Ruling (<https://arkhamdb.com/card/01051>):
//!
//! > Backstab makes you test your Agility skill instead of your Combat skill
//! > against the enemy's Fight value (not its Evade value).

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, EnemyId, GameState, GameStateBuilder, InvestigatorId,
    LocationId, SkillKind,
};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event, assert_no_event};

const BACKSTAB: &str = "01051";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

const PLAY: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// Backstab in hand with the resources to play it, the investigator at
/// agility `agility` and combat **1** with 3 actions, engaged with a fight-3,
/// evade-5, health-4 enemy whose attack deals 1 damage. A `Numeric(0)` bag.
///
/// Agility 4 hits fight 3 but would miss evade 5, and combat 1 misses fight 3,
/// so the outcome says both which skill and which enemy value the test used.
fn board(agility: i8) -> GameState {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
    enemy.fight = 3;
    enemy.evade = 5;
    enemy.max_health = 4;

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Agility, agility)
                .skill(SkillKind::Combat, 1)
                .hand([BACKSTAB])
                .resources(3)
                .actions(3),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build()
}

/// Play Backstab against the one engaged enemy, committing nothing, and come
/// back to the turn menu.
fn play(agility: i8) -> TestSession {
    let session = TestSession::new(board(agility), TEST_REGISTRIES)
        .take(&PLAY)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn the_attack_tests_agility_against_fight_and_deals_two_extra_damage() {
    // Agility 4 vs fight 3 succeeds by 1: combat 1 would fail, and against
    // evade 5 the test would fail too. The hit deals 1 + 2 = 3.
    let session = play(4);
    assert_event!(
        session.events(),
        Event::SkillTestStarted {
            skill: SkillKind::Agility,
            difficulty: 3,
            ..
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded {
            skill: SkillKind::Agility,
            margin: 1,
            ..
        }
    );
    assert_event!(session.events(), Event::EnemyDamaged { amount: 3, .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 3);
    // The play is the one action spent, it provokes no attack of opportunity,
    // and the event is discarded.
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(BACKSTAB)]
    );
}

#[test]
fn a_failed_attack_deals_no_damage() {
    // Agility 2 vs fight 3 fails by 1.
    let session = play(2);
    assert_event!(
        session.events(),
        Event::SkillTestFailed {
            skill: SkillKind::Agility,
            by: 1,
            ..
        }
    );
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
    assert_eq!(session.state().enemies[&ENEMY].damage, 0);
}
