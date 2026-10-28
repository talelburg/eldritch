//! Cunning Distraction 01078 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01078):
//!
//! > <b>Evade.</b> Automatically evade all enemies at your location.
//!
//! Rulings (<https://arkhamdb.com/card/01078>), in part:
//!
//! > This effect will evade and exhaust all enemies in your location, even
//! > those that you're not engaged with (***Aloof***, and those engaged with
//! > other players).
//!
//! > […] if an ability automatically evades 1 or more enemies, no skill test is
//! > made for the evasion attempt whatsoever.
//!
//! Aloof is not modelled, so the Aloof clause is tested through what it means
//! for the board: an enemy at your location engaged with no one.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    CardCode, Enemy, EnemyId, GameStateBuilder, InvestigatorId, LocationId, Zone,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

const CUNNING_DISTRACTION: &str = "01078";
const PICKPOCKETING: &str = "01046";
/// Deck filler, the card Pickpocketing draws.
const GUTS: &str = "01089";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const ELSEWHERE: LocationId = LocationId(11);
/// Engaged with investigator 1.
const ENGAGED: EnemyId = EnemyId(100);
/// At `HERE`, engaged with no one.
const UNENGAGED: EnemyId = EnemyId(101);
/// At `HERE`, engaged with investigator 2.
const OTHERS: EnemyId = EnemyId(102);
/// At `ELSEWHERE`.
const AWAY: EnemyId = EnemyId(103);

/// A ready enemy dealing 1 damage, standing at `at`.
fn enemy(id: EnemyId, at: LocationId) -> Enemy {
    let mut e = test_support::test_enemy(id.0, "Ghoul");
    e.attack_damage = 1;
    e.attack_horror = 0;
    e.current_location = Some(at);
    e
}

/// Investigator `id` at `HERE` with 3 actions and 5 resources.
fn survivor(id: u32) -> Seat {
    seat(id).at(HERE).actions(3).resources(5)
}

/// The two-location board on investigator 1's open turn: investigator 1 holds
/// Cunning Distraction, investigator 2 stands beside them, and the enemies are
/// placed as their constants say. `include` picks which of them are in play.
fn board(first: Seat, include: &[EnemyId]) -> TestSession {
    let mut builder = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_location(test_support::test_location(ELSEWHERE.0, "Hallway"))
        .with_seat(first.hand([CUNNING_DISTRACTION]))
        .with_seat(survivor(2));
    for &id in include {
        builder = match id {
            ENGAGED => builder.with_enemy_engaged(enemy(id, HERE), INV),
            OTHERS => builder.with_enemy_engaged(enemy(id, HERE), INV2),
            AWAY => builder.with_enemy(enemy(id, ELSEWHERE)),
            _ => builder.with_enemy(enemy(id, HERE)),
        };
    }
    builder.open_turn(INV).session()
}

fn play() -> TurnAction {
    TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    }
}

#[test]
fn it_evades_and_exhausts_every_enemy_at_your_location_engaged_or_not() {
    let s = board(survivor(1), &[ENGAGED, UNENGAGED, OTHERS, AWAY]).take(&play());

    s.assert_at_turn_menu(INV);
    for id in [ENGAGED, UNENGAGED, OTHERS] {
        let e = &s.state().enemies[&id];
        assert!(e.exhausted, "{id:?} is exhausted");
        assert_eq!(e.engaged_with, None, "{id:?} is engaged with no one");
        assert_event!(
            s.events(),
            Event::EnemyEvaded { enemy, investigator } if *enemy == id && *investigator == INV
        );
    }
    let away = &s.state().enemies[&AWAY];
    assert!(!away.exhausted, "an enemy elsewhere is not evaded");
    assert_event_count!(s.events(), 3, Event::EnemyEvaded { .. });
    assert_event!(
        s.events(),
        Event::CardDiscarded { from: Zone::Hand, code, .. } if *code == CardCode::new(CUNNING_DISTRACTION)
    );
}

#[test]
fn no_skill_test_is_made_for_the_evasion() {
    let s = board(survivor(1), &[UNENGAGED]).take(&play());

    assert_event!(s.events(), Event::EnemyEvaded { .. });
    assert_no_event!(s.events(), Event::SkillTestStarted { .. });
    assert_no_event!(s.events(), Event::SkillTestSucceeded { .. });
}

#[test]
fn as_an_evade_it_provokes_no_attack_of_opportunity() {
    let s = board(survivor(1), &[ENGAGED]).take(&play());

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].actions_remaining,
        2,
        "the play is the one action spent",
    );
    assert_no_event!(s.events(), Event::DamagePlacedOnInvestigator { .. });
}

#[test]
fn with_no_enemy_at_your_location_it_cannot_be_played() {
    let s = board(survivor(1), &[AWAY]);
    assert!(
        !enumerate::legal_actions(s.state(), TEST_REGISTRIES.cards).contains(&play()),
        "not offered with nothing to evade",
    );

    s.assert_rejects(|s| s.submit_off_menu(&play()));
}

#[test]
fn pickpocketing_reacts_after_the_first_evasion_before_the_second() {
    let s = board(
        survivor(1).in_play([PICKPOCKETING]).deck([GUTS]),
        &[ENGAGED, UNENGAGED],
    )
    .take(&play());

    // One `EnemyEvaded` per enemy, in id order, each with its own "after"
    // window: the first evasion has resolved and the second has not.
    let pickpocketing = s.state().card(INV, PICKPOCKETING);
    assert_eq!(s.offered_at(&pickpocketing), 1, "offered after an evasion");
    assert!(s.state().enemies[&ENGAGED].exhausted);
    assert!(!s.state().enemies[&UNENGAGED].exhausted);

    let s = s.pick(pickpocketing);
    s.assert_at_turn_menu(INV);
    assert!(s.state().enemies[&UNENGAGED].exhausted, "the walk resumed");
    assert_eq!(s.state().investigators[&INV].hand, [CardCode::new(GUTS)]);
}
