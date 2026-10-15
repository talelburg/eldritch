//! Sneak Attack 01052 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01052):
//!
//! > Deal 2 damage to an exhausted enemy at your location.
//!
//! Ruling (<https://arkhamdb.com/card/01052>):
//!
//! > During your turn, you exhaust an enemy by evading it, then play Sneak
//! > Attack. You do not play it during the enemy phase.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{CardCode, Enemy, EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event, assert_no_event};

const SNEAK_ATTACK: &str = "01052";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

const PLAY: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    hand_index: 0,
};

/// An unengaged 5-health enemy at `at`, exhausted or ready.
fn enemy(id: u32, at: LocationId, exhausted: bool) -> Enemy {
    let mut enemy = test_support::test_enemy(id, "Ghoul");
    enemy.max_health = 5;
    enemy.current_location = Some(at);
    enemy.exhausted = exhausted;
    enemy
}

/// Sneak Attack in hand with the resources to play it, at `HERE` with a
/// second location `THERE`, and `enemies` on the board.
fn board(enemies: impl IntoIterator<Item = Enemy>) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([SNEAK_ATTACK])
                .resources(2)
                .actions(3),
        )
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_location(test_support::test_location(THERE.0, "There"));
    for enemy in enemies {
        builder = builder.with_enemy(enemy);
    }
    builder.open_turn(INV).session()
}

fn damage(session: &TestSession, enemy: EnemyId) -> u8 {
    session.state().enemies[&enemy].damage
}

#[test]
fn only_exhausted_enemies_at_your_location_are_offered_and_the_chosen_one_takes_two() {
    let (a, b) = (EnemyId(100), EnemyId(101));
    let (ready, away) = (EnemyId(102), EnemyId(103));
    let session = board([
        enemy(a.0, HERE, true),
        enemy(b.0, HERE, true),
        enemy(ready.0, HERE, false),
        enemy(away.0, THERE, true),
    ])
    .take(&PLAY);

    assert_eq!(session.offered_at(&OptionTarget::Enemy(a)), 1);
    assert_eq!(session.offered_at(&OptionTarget::Enemy(b)), 1);
    assert_eq!(
        session.offered_at(&OptionTarget::Enemy(ready)),
        0,
        "a ready enemy at your location is not offered"
    );
    assert_eq!(
        session.offered_at(&OptionTarget::Enemy(away)),
        0,
        "an exhausted enemy elsewhere is not offered"
    );

    let session = session.pick(OptionTarget::Enemy(b));
    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::EnemyDamaged { enemy, amount: 2, .. } if *enemy == b
    );
    assert_eq!(damage(&session, b), 2);
    assert_eq!(damage(&session, a), 0);
    assert_eq!(damage(&session, ready), 0);
    assert_eq!(damage(&session, away), 0);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.resources, 0, "cost 2 paid");
    assert_eq!(inv.discard, vec![CardCode::new(SNEAK_ATTACK)]);
}

#[test]
fn a_lone_exhausted_enemy_at_your_location_takes_two() {
    let target = EnemyId(100);
    let session = board([enemy(target.0, HERE, true)]).take(&PLAY);

    session.assert_at_turn_menu(INV);
    assert_eq!(damage(&session, target), 2);
}

#[test]
fn not_playable_with_no_exhausted_enemy_at_your_location() {
    // A ready enemy here and an exhausted one elsewhere: neither qualifies,
    // so the effect cannot change the game state and the play is not offered.
    let session = board([enemy(100, HERE, false), enemy(101, THERE, true)]);
    assert!(
        !enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&PLAY),
        "Sneak Attack must not be on the turn menu"
    );
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
}
