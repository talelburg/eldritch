//! Integration: Dynamite Blast 01024's `Choose either your
//! location or a connecting location. Deal 3 damage to each enemy and to each
//! investigator at the chosen location.` end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Exercises (a) the location choice — auto when there's one candidate, a
//! suspend/resume `Continuation::Choice` when there are 2+; (b) the area
//! damage over both enemies and investigators at the chosen location (and *not* the other
//! location); (c) self-damage when blasting your own location; (d) the
//! played-event being discarded on *completion* of a suspending `OnPlay`
//! (RR Appendix I step 4) rather than stranded in hand — it rides its
//! `Continuation::PlayFromHand` frame in the meantime.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    CardCode, EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const DYNAMITE: &str = "01024";
const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const HALLWAY: LocationId = LocationId(11);
const ENEMY_A: EnemyId = EnemyId(100);
const ENEMY_B: EnemyId = EnemyId(101);

fn play(state: GameState) -> TestSession {
    TestSession::new(state, TEST_REGISTRIES).take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    })
}

/// Two connected locations. The controller (Dynamite in hand) and a 3-health
/// enemy sit at `HERE`; a second investigator and a 5-health enemy sit at the
/// connecting `HALLWAY`.
fn board() -> GameState {
    let mut loc_a = test_support::test_location(HERE.0, "Cellar");
    loc_a.connections = vec![HALLWAY];
    let mut loc_b = test_support::test_location(HALLWAY.0, "Hallway");
    loc_b.connections = vec![HERE];

    let mut enemy_a = test_support::test_enemy(ENEMY_A.0, "Ghoul A");
    enemy_a.current_location = Some(HERE);
    enemy_a.max_health = 3; // 3 damage defeats it
    let mut enemy_b = test_support::test_enemy(ENEMY_B.0, "Ghoul B");
    enemy_b.current_location = Some(HALLWAY);
    enemy_b.max_health = 5; // survives, to assert it's untouched

    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).hand([DYNAMITE]))
        .with_seat(seat(2).at(HALLWAY))
        .with_location(loc_a)
        .with_location(loc_b)
        .with_enemy(enemy_a)
        .with_enemy(enemy_b)
        .open_turn(INV)
        .build()
}

#[test]
fn blasts_only_the_chosen_location_then_discards_the_event() {
    // Two candidates (your location + the connection) → suspend.
    let s = play(board());
    assert_eq!(
        (
            s.offered_at(&OptionTarget::Location(HERE)),
            s.offered_at(&OptionTarget::Location(HALLWAY)),
        ),
        (1, 1),
        "2 candidate locations → choice suspends: {:?}",
        s.prompt(),
    );
    // Each option names its location, not the `Debug` form of its id.
    let labels: Vec<_> = s
        .prompt()
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect();
    assert_eq!(labels, ["Cellar", "Hallway"]);
    // The event has left hand ("commences being played") but isn't discarded yet.
    assert!(
        s.state().investigators[&INV].hand.is_empty(),
        "event left hand"
    );
    assert!(
        s.state().investigators[&INV].discard.is_empty(),
        "not discarded until the effect completes",
    );
    assert_eq!(
        s.state().play_in_progress().map(|(_, c)| c.clone()),
        Some(CardCode::new(DYNAMITE)),
        "the event is mid-play, held by its frame",
    );

    // Blast the controller's own location.
    let s = s.pick(OptionTarget::Location(HERE));
    s.assert_at_turn_menu(INV);

    // HERE: enemy defeated (3 dmg ≥ 3 health), controller took 3 (self-damage).
    assert!(
        !s.state().enemies.contains_key(&ENEMY_A),
        "enemy at the blasted location was defeated and removed",
    );
    assert_eq!(
        s.state().investigators[&INV].damage(),
        3,
        "the controller blasted its own location and took 3",
    );
    // HALLWAY (not chosen): untouched.
    assert_eq!(
        s.state().enemies[&ENEMY_B].damage,
        0,
        "enemy at HALLWAY untouched"
    );
    assert_eq!(
        s.state().investigators[&INV2].damage(),
        0,
        "investigator at HALLWAY untouched",
    );

    // Discard-on-completion (RR Appendix I step 4): the event is now discarded.
    assert_eq!(
        s.state().investigators[&INV].discard,
        vec![CardCode::new(DYNAMITE)],
        "event discarded when its effect completed",
    );
    assert!(
        s.state().play_in_progress().is_none(),
        "no play left in progress once the card is placed",
    );
}

#[test]
fn auto_targets_and_discards_when_your_location_is_the_only_candidate() {
    // A single location with no connections → one candidate → auto, no suspend.
    let loc_a = test_support::test_location(HERE.0, "Cellar"); // no connections
    let mut enemy_a = test_support::test_enemy(ENEMY_A.0, "Ghoul A");
    enemy_a.current_location = Some(HERE);
    enemy_a.max_health = 5; // survives, to assert the 3-damage amount

    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).hand([DYNAMITE]))
        .with_location(loc_a)
        .with_enemy(enemy_a)
        .open_turn(INV)
        .build();

    let s = play(state);
    // Returns to the open-turn menu; the damage assertions below prove the
    // blast resolved fully (a single candidate auto-binds — no target-pick
    // suspend).
    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().enemies[&ENEMY_A].damage, 3, "enemy took 3");
    assert_eq!(
        s.state().investigators[&INV].damage(),
        3,
        "controller took 3"
    );
    assert_eq!(
        s.state().investigators[&INV].discard,
        vec![CardCode::new(DYNAMITE)],
        "event discarded",
    );
}

#[test]
fn blast_is_rejected_when_the_controller_has_no_location() {
    // Between locations, there is neither "your location" nor a connecting
    // one to choose, so the empty candidate list rejects the blast.
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand([DYNAMITE]))
        .with_location(test_support::test_location(HERE.0, "Cellar"))
        .open_turn(INV)
        .build();
    assert_eq!(state.investigators[&INV].current_location, None);

    let s = play(state);
    assert_eq!(
        s.expect_rejected(),
        "01024 blast: controller has no location to target",
    );
    // The rejection rolls the play back: the event is still in hand, nothing
    // was discarded, and the session rests at the turn menu.
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(DYNAMITE)],
        "event back in hand",
    );
    assert!(s.state().investigators[&INV].discard.is_empty());
}

/// Guard Dog 01021: health 3, sanity 1. The health-bearing soaker.
const GUARD_DOG: &str = "01021";

/// The blast's investigator damage is dealt the way an enemy attack's is: a
/// health-bearing asset is offered the soak, one point at a time,
/// rather than absorbing the damage silently.
#[test]
fn blasted_investigator_is_offered_the_soak() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).hand([DYNAMITE]).in_play([GUARD_DOG]))
        .with_location(test_support::test_location(HERE.0, "Cellar"))
        .open_turn(INV)
        .build();
    let dog = state.in_play(INV, GUARD_DOG);

    let s = play(state);
    let dog_target = OptionTarget::CardInstance(dog);
    let me = s.state().investigators[&INV].card_anchor();
    assert_eq!((s.offered_at(&dog_target), s.offered_at(&me)), (1, 1));
    assert_eq!(
        s.state().investigators[&INV].damage(),
        0,
        "nothing placed yet"
    );

    // One point on the dog, two on the investigator: the player's split, not
    // the soak-first default.
    let s = s.pick(dog_target).pick(me.clone()).pick(me);
    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().investigators[&INV].damage(), 2);
    let dog_card = s.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == dog)
        .expect("Guard Dog still in play");
    assert_eq!(dog_card.accumulated_damage, 1);
}

/// With two investigators at the chosen location, each is dealt their own
/// damage in ascending id order, the first deal completing before the second
/// begins: investigator 1's damage is placed while investigator 2's soak
/// prompt is still open.
#[test]
fn two_blasted_investigators_are_dealt_one_at_a_time_in_ascending_order() {
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).at(HERE).hand([DYNAMITE]))
        .with_seat(seat(2).at(HERE).in_play([GUARD_DOG]))
        .with_location(test_support::test_location(HERE.0, "Cellar"))
        .open_turn(INV)
        .build();
    let dog = OptionTarget::CardInstance(state.in_play(INV2, GUARD_DOG));

    let s = play(state);
    assert_eq!(s.offered_at(&dog), 1, "investigator 2's soak prompt");
    assert_eq!(
        s.state().investigators[&INV].damage(),
        3,
        "investigator 1's deal completed first"
    );
    assert_eq!(s.state().investigators[&INV2].damage(), 0);

    let me2 = s.state().investigators[&INV2].card_anchor();
    let s = s.pick(me2.clone()).pick(me2.clone()).pick(me2);
    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().investigators[&INV2].damage(), 3);
    let taken: Vec<InvestigatorId> = s
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::DamageTaken { investigator, .. } => Some(*investigator),
            _ => None,
        })
        .collect();
    assert_eq!(taken, vec![INV, INV2]);
}
