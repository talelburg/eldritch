//! PR-8 (#306) integration: Dynamite Blast 01024's `Choose either your
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
//! Own process → installs `cards::REGISTRY`.

use cards::REGISTRY;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{
    CardCode, EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId,
};
use game_core::test_support::{self, TestSession};

const DYNAMITE: &str = "01024";
const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const LOC_A: LocationId = LocationId(10);
const LOC_B: LocationId = LocationId(11);
const ENEMY_A: EnemyId = EnemyId(100);
const ENEMY_B: EnemyId = EnemyId(101);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

fn play(state: GameState) -> TestSession {
    TestSession::new(state).take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    })
}

fn at_turn_menu(session: &TestSession) -> bool {
    session.prompt().target == Some(OptionTarget::TurnControl(INV))
}

/// Two connected locations. The controller (Dynamite in hand) and a 3-health
/// enemy sit at `LOC_A`; a second investigator and a 5-health enemy sit at the
/// connecting `LOC_B`.
fn board() -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LOC_A);
    inv.hand = vec![CardCode::new(DYNAMITE)];

    let mut inv2 = test_support::test_investigator(2);
    inv2.current_location = Some(LOC_B);
    // Same reason as inv above.

    let mut loc_a = test_support::test_location(10, "Cellar");
    loc_a.connections = vec![LOC_B];
    let mut loc_b = test_support::test_location(11, "Hallway");
    loc_b.connections = vec![LOC_A];

    let mut enemy_a = test_support::test_enemy(100, "Ghoul A");
    enemy_a.current_location = Some(LOC_A);
    enemy_a.max_health = 3; // 3 damage defeats it
    let mut enemy_b = test_support::test_enemy(101, "Ghoul B");
    enemy_b.current_location = Some(LOC_B);
    enemy_b.max_health = 5; // survives, to assert it's untouched

    GameStateBuilder::new()
        .with_investigator(inv)
        .with_investigator(inv2)
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
    assert!(!at_turn_menu(&s), "2 candidate locations → choice suspends");
    // Each option names its location (#989), not the `Debug` form of its id.
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

    // candidate_locations = [LOC_A, LOC_B] → option 0 blasts LOC_A.
    let s = s.pick(OptionTarget::Location(LOC_A));
    assert!(at_turn_menu(&s));

    // LOC_A: enemy defeated (3 dmg ≥ 3 health), controller took 3 (self-damage).
    assert!(
        !s.state().enemies.contains_key(&ENEMY_A),
        "enemy at the blasted location was defeated and removed",
    );
    assert_eq!(
        s.state().investigators[&INV].damage(),
        3,
        "the controller blasted its own location and took 3",
    );
    // LOC_B (not chosen): untouched.
    assert_eq!(
        s.state().enemies[&ENEMY_B].damage,
        0,
        "enemy at LOC_B untouched"
    );
    assert_eq!(
        s.state().investigators[&INV2].damage(),
        0,
        "investigator at LOC_B untouched",
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
    let mut inv = test_support::test_investigator(1);
    inv.current_location = Some(LOC_A);
    inv.hand = vec![CardCode::new(DYNAMITE)];

    let loc_a = test_support::test_location(10, "Cellar"); // no connections
    let mut enemy_a = test_support::test_enemy(100, "Ghoul A");
    enemy_a.current_location = Some(LOC_A);
    enemy_a.max_health = 5; // survives, to assert the 3-damage amount

    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(loc_a)
        .with_enemy(enemy_a)
        .open_turn(INV)
        .build();

    let s = play(state);
    // Returns to the open-turn menu; the damage assertions below prove the
    // blast resolved fully (a single candidate auto-binds — no target-pick
    // suspend).
    assert!(at_turn_menu(&s));
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
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new(DYNAMITE)];
    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .with_location(test_support::test_location(10, "Cellar"))
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
    assert!(at_turn_menu(&s));
    assert_eq!(
        s.state().investigators[&INV].hand,
        vec![CardCode::new(DYNAMITE)],
        "event back in hand",
    );
    assert!(s.state().investigators[&INV].discard.is_empty());
}
