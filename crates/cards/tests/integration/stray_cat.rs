//! Stray Cat 01076 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01076):
//!
//! > [fast] Discard Stray Cat: Automatically evade a non-[[Elite]] enemy at
//! > your location.
//!
//! Rulings (<https://arkhamdb.com/card/01076>), in part:
//!
//! > You can use Stray Cat to evade an enemy that you're not engaged with.
//!
//! > […] if an ability automatically evades 1 or more enemies, no skill test is
//! > made for the evasion attempt whatsoever. Consequentially, because no skill
//! > test is made, it is not considered a “successful” evasion.
//!
//! Pickpocketing 01046's ruling (<https://arkhamdb.com/card/01046>) says an
//! automatic evasion still triggers it: *"the auto-evade effect from Breaking
//! and Entering can trigger the reaction on Pickpocketing (0)."*
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, Enemy, EnemyId, GameStateBuilder,
    InvestigatorId, LocationId, Zone,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const STRAY_CAT: &str = "01076";
const PICKPOCKETING: &str = "01046";
/// Ghoul Priest (01116): *Humanoid. Monster. Ghoul. Elite.*
const GHOUL_PRIEST: &str = "01116";
/// Ghoul Minion (01160): *Humanoid. Monster. Ghoul.*, not Elite.
const GHOUL_MINION: &str = "01160";
/// Deck filler, the card Pickpocketing draws.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ELSEWHERE: LocationId = LocationId(11);
const MINION: EnemyId = EnemyId(100);
const PRIEST: EnemyId = EnemyId(101);

/// A ready, unengaged enemy printed as `code`, standing at `at`.
fn enemy(id: EnemyId, code: &str, at: LocationId) -> Enemy {
    let mut e = test_support::test_enemy(id.0, code);
    e.code = CardCode::new(code);
    e.current_location = Some(at);
    e
}

/// Investigator 1 at `HERE` with Stray Cat in play.
fn survivor() -> Seat {
    seat(1).at(HERE).in_play([STRAY_CAT])
}

/// `seat`'s board on investigator 1's open turn, with `enemies` placed
/// unengaged where each says.
fn board(seat: Seat, enemies: Vec<Enemy>) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_location(test_support::test_location(ELSEWHERE.0, "Hallway"))
        .with_seat(seat);
    for e in enemies {
        builder = builder.with_enemy(e);
    }
    builder.open_turn(INV).session()
}

/// Stray Cat's `[fast]` discard-to-evade ability, its only one.
fn activate(session: &TestSession) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, STRAY_CAT)),
        address: AbilityAddress::Printed(0),
    }
}

#[test]
fn it_evades_an_unengaged_non_elite_enemy_at_your_location() {
    let s = board(survivor(), vec![enemy(MINION, GHOUL_MINION, HERE)]);
    let action = activate(&s);
    let s = s.take(&action);

    s.assert_at_turn_menu(INV);
    let minion = &s.state().enemies[&MINION];
    assert!(minion.exhausted, "the evaded enemy is exhausted");
    assert_eq!(minion.engaged_with, None);
    assert_event!(
        s.events(),
        Event::EnemyEvaded { enemy, investigator } if *enemy == MINION && *investigator == INV
    );
    assert_event!(
        s.events(),
        Event::CardDiscarded {
            from: Zone::InPlay,
            to: DiscardPile::Investigator(INV),
            ..
        }
    );
    assert_eq!(
        s.state().investigators[&INV].discard,
        [CardCode::new(STRAY_CAT)],
        "Stray Cat paid its own discard",
    );
}

#[test]
fn no_skill_test_is_made_for_the_evasion() {
    let s = board(survivor(), vec![enemy(MINION, GHOUL_MINION, HERE)]);
    let action = activate(&s);
    let s = s.take(&action);

    assert_event!(s.events(), Event::EnemyEvaded { .. });
    assert_no_event!(s.events(), Event::SkillTestStarted { .. });
    assert_no_event!(s.events(), Event::SkillTestSucceeded { .. });
}

#[test]
fn it_evades_an_enemy_engaged_with_you_and_breaks_the_engagement() {
    let s = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(survivor())
        .with_enemy_engaged(enemy(MINION, GHOUL_MINION, HERE), INV)
        .open_turn(INV)
        .session();
    let action = activate(&s);
    let s = s.take(&action);

    let minion = &s.state().enemies[&MINION];
    assert_eq!(minion.engaged_with, None, "the engagement is broken");
    assert!(minion.exhausted);
    assert_event_sequence!(
        s.events(),
        Event::EnemyEvaded { enemy, .. } if *enemy == MINION,
        Event::EnemyDisengaged { .. },
        Event::EnemyExhausted { enemy } if *enemy == MINION,
    );
}

#[test]
fn it_cannot_target_an_elite_enemy() {
    let s = board(survivor(), vec![enemy(PRIEST, GHOUL_PRIEST, HERE)]);
    let action = activate(&s);
    assert!(
        !enumerate::legal_actions(s.state(), TEST_REGISTRIES.cards).contains(&action),
        "not offered with only an Elite enemy at your location",
    );

    // The target check runs before the discard is paid: nothing moves.
    s.assert_rejects(|s| s.submit_off_menu(&action));
}

#[test]
fn beside_an_elite_enemy_the_non_elite_one_is_the_only_target() {
    let s = board(
        survivor(),
        vec![
            enemy(MINION, GHOUL_MINION, HERE),
            enemy(PRIEST, GHOUL_PRIEST, HERE),
        ],
    );
    let action = activate(&s);
    // A lone candidate is evaded with no prompt.
    let s = s.take(&action);

    s.assert_at_turn_menu(INV);
    assert!(s.state().enemies[&MINION].exhausted);
    assert!(
        !s.state().enemies[&PRIEST].exhausted,
        "the Elite is untouched"
    );
}

#[test]
fn an_enemy_at_another_location_is_not_a_target() {
    let s = board(survivor(), vec![enemy(MINION, GHOUL_MINION, ELSEWHERE)]);
    let action = activate(&s);
    assert!(!enumerate::legal_actions(s.state(), TEST_REGISTRIES.cards).contains(&action));
}

#[test]
fn two_non_elite_enemies_ask_which_to_evade() {
    const SECOND: EnemyId = EnemyId(102);
    let s = board(
        survivor(),
        vec![
            enemy(MINION, GHOUL_MINION, HERE),
            enemy(SECOND, GHOUL_MINION, HERE),
        ],
    );
    let action = activate(&s);
    let s = s.take(&action).pick(OptionTarget::Enemy(SECOND));

    s.assert_at_turn_menu(INV);
    assert!(s.state().enemies[&SECOND].exhausted);
    assert!(!s.state().enemies[&MINION].exhausted);
}

#[test]
fn pickpocketing_reacts_to_the_automatic_evasion() {
    let s = board(
        survivor().in_play([PICKPOCKETING]).deck([GUTS]),
        vec![enemy(MINION, GHOUL_MINION, HERE)],
    );
    let action = activate(&s);
    let s = s.take(&action);

    let pickpocketing = s.state().card(INV, PICKPOCKETING);
    assert_eq!(s.offered_at(&pickpocketing), 1, "offered after the evasion");
    let s = s.pick(pickpocketing);
    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().investigators[&INV].hand, [CardCode::new(GUTS)]);
}
