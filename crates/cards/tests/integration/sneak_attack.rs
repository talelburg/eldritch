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
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{enemy, location, seat, StagedEnemy, TestSession};
use game_core::{assert_event, assert_no_event};

const SNEAK_ATTACK: &str = "01052";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

const PLAY: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// Sneak Attack in hand with the resources to play it, at `HERE` with a
/// second location `THERE`, and `enemies` (unengaged) on the board.
fn board(enemies: impl IntoIterator<Item = StagedEnemy>) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([SNEAK_ATTACK])
                .resources(2)
                .actions(3),
        )
        .with_location(location(HERE))
        .with_location(location(THERE));
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
        enemy(a).health(5).at(HERE).exhausted(),
        enemy(b).health(5).at(HERE).exhausted(),
        enemy(ready).health(5).at(HERE),
        enemy(away).health(5).at(THERE).exhausted(),
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
    let session = board([enemy(target).health(5).at(HERE).exhausted()]).take(&PLAY);

    session.assert_at_turn_menu(INV);
    assert_eq!(damage(&session, target), 2);
}

#[test]
fn not_playable_with_no_exhausted_enemy_at_your_location() {
    // A ready enemy here and an exhausted one elsewhere: neither qualifies,
    // so the effect cannot change the game state and the play is not offered.
    let session = board([
        enemy(100).health(5).at(HERE),
        enemy(101).health(5).at(THERE).exhausted(),
    ]);
    assert!(
        !enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&PLAY),
        "Sneak Attack must not be on the turn menu"
    );
    assert_no_event!(session.events(), Event::EnemyDamaged { .. });
}
