//! Reactions and elimination.
//!
//! `glossary/Elimination.md`: *"The only manner in which eliminated
//! investigators interact with the game is when establishing "per investigator"
//! values"* — so an eliminated investigator is never offered a reaction, and
//! the initiation gate refuses a controller who is not Active (ADR 0017). The
//! act's and agenda's reactions bind the lead-investigator proxy, the first Active
//! investigator in `turn_order` (GLOSSARY "Lead investigator"), so they survive
//! the first seat's elimination.
//!
//! Integration test so it can run against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, TimingEvent};
use game_core::state::{
    Act, CardCode, EnemyId, GameStateBuilder, InvestigatorId, Location, LocationId, Status,
};
use game_core::test_support::{self, seat, TestSession};

const SEAT_1: InvestigatorId = InvestigatorId(1);
const SEAT_2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const HALLWAY: LocationId = LocationId(2);

/// Two seats at a Study with a clue on it, in seat 2's turn; seat 1 is Roland
/// Banks (01001) and has `status`. 01001: *"\[reaction\] After you defeat an
/// enemy: Discover 1 clue at your location. (Limit once per round.)"*
///
/// Roland defeats an enemy by firing the timing point over seat 2's open turn:
/// a defeated Roland cannot take the action that defeats one, and the control
/// must reach the window the same way for only his status to separate the two.
fn roland_defeats_an_enemy(status: Status) -> TestSession {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.clues = 1;
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).as_card("01001").at(HERE).status(status))
        .with_seat(seat(2).at(HERE))
        .with_location(study)
        .with_turn_order([SEAT_1, SEAT_2])
        .open_turn(SEAT_2)
        .session()
        .fire_at(TimingEvent::EnemyDefeated {
            enemy: EnemyId(100),
            by: Some(SEAT_1),
            code: CardCode::new("01160"),
        })
}

/// Control for the test below: the same table with Roland Active offers his
/// reaction, so only his status separates the two.
#[test]
fn an_active_roland_is_offered_his_reaction() {
    let session = roland_defeats_an_enemy(Status::Active);
    let rolands_card = session.state().investigators[&SEAT_1].card_anchor();
    assert_eq!(
        (
            session.offered_at(&rolands_card),
            session.prompt().options.len()
        ),
        (1, 1),
        "the window offers Roland's own reaction alone: {:?}",
        session.prompt(),
    );
}

/// Elimination leaves the investigator card behind (it cannot be drained), so
/// a defeated Roland still holds his printed reaction; the gate's
/// status check is what keeps it from being offered.
#[test]
fn an_eliminated_investigator_is_offered_no_reaction() {
    // No window opened: play comes straight back to seat 2's turn menu.
    roland_defeats_an_enemy(Status::Defeated).assert_at_turn_menu(SEAT_2);
}

/// The Barrier (01109) is the current act, and seat 2 stands in the Hallway
/// (01112) holding the act's clue threshold, in their turn. Seat 1 has been
/// eliminated. 01109: *"Objective - When the round ends, investigators in the
/// hallway may, as a group, spend the requisite number of clues to advance."*
fn barrier_table_with_seat_1_eliminated() -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).status(Status::Defeated))
        // A spare deck card so Upkeep step 4.4's draw has something to take.
        .with_seat(seat(2).at(HALLWAY).clues(3).deck(["01088"]))
        .with_location(Location::new(
            HALLWAY,
            CardCode::new("01112"),
            "Hallway",
            1,
            0,
        ))
        .with_turn_order([SEAT_1, SEAT_2])
        .open_turn(SEAT_2)
        .build();
    // The builder stages no act deck.
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 3,
    }];
    state.act_index = 0;
    TestSession::new(state, TEST_REGISTRIES)
}

#[test]
fn an_act_reaction_survives_the_first_seat_being_eliminated() {
    // Seat 2's turn is the round's last, so ending it runs on to the round end.
    let session = barrier_table_with_seat_1_eliminated().take(&TurnAction::EndTurn);
    // The initiation gate offers a reaction only to an Active controller, and
    // seat 2 is the one Active investigator: the act's reaction is offered, so
    // it is controlled by seat 2, the first Active investigator.
    assert_eq!(
        (
            session.offered_at(&OptionTarget::Act),
            session.prompt().options.len()
        ),
        (1, 1),
        "the act's reaction is controlled by the first Active investigator: {:?}",
        session.prompt(),
    );
}
