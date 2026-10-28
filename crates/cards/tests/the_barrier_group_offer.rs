//! The Barrier 01109's round-end clue spend is offered to an investigator in
//! the Hallway, not to a lead standing elsewhere.
//!
//! 01109, `text` verbatim (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//! *"**Objective** - When the round ends, investigators in the hallway may, as
//! a group, spend the requisite number of clues to advance."* It has no rulings
//! (`data/arkhamdb-faq/no-rulings.txt`).
//!
//! An act's reaction binds the lead unless the ability names who may act (ADR
//! 0019); this one names the investigators in the Hallway, so the offer goes to
//! the first of them in player order, and the group spends in that order.
//!
//! Each Hallway here holds Roland Banks (01001, combat 4) and at most Daisy
//! Walker (01002, combat 2), so the Ghoul Priest's *"Prey - Highest
//! \[combat\]"* spawn on the reverse has no tie to break.
//!
//! Integration test so it can run against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{
    Act, CardCode, GameState, GameStateBuilder, InvestigatorId, Location, LocationId,
};
use game_core::test_support::{seat, Seat, TestSession};

const SEAT_1: InvestigatorId = InvestigatorId(1);
const SEAT_2: InvestigatorId = InvestigatorId(2);
const SEAT_3: InvestigatorId = InvestigatorId(3);
const HALLWAY: LocationId = LocationId(1);
const PARLOR: LocationId = LocationId(2);
const ATTIC: LocationId = LocationId(3);

/// The Barrier current with The Ghoul Priest (01110) behind it, the Hallway,
/// Parlor and Attic in play, and the Ghoul Priest and Lita Chantler set aside
/// for the reverse. Each seat gets a spare deck card for Upkeep 4.4's draw.
/// `lead` holds the lead; `last` has the round's last turn open, so ending it
/// runs on to the round end.
fn barrier_table(
    seats: Vec<Seat>,
    turn_order: &[InvestigatorId],
    lead: InvestigatorId,
    last: InvestigatorId,
) -> TestSession {
    let mut builder = GameStateBuilder::new().with_registries(TEST_REGISTRIES);
    for s in seats {
        builder = builder.with_seat(s.deck(["01088"]));
    }
    let mut state: GameState = builder
        .with_location(Location::new(
            HALLWAY,
            CardCode::new("01112"),
            "Hallway",
            1,
            0,
        ))
        .with_location(Location::new(
            PARLOR,
            CardCode::new("01115"),
            "Parlor",
            3,
            0,
        ))
        .with_location(Location::new(ATTIC, CardCode::new("01113"), "Attic", 1, 0))
        .with_turn_order(turn_order.iter().copied())
        .with_lead(lead)
        .open_turn(last)
        .build();
    state.act_deck = ["01109", "01110"]
        .into_iter()
        .map(|code| Act {
            code: CardCode::new(code),
            clue_threshold: 3,
        })
        .collect();
    state.act_index = 0;
    state.set_aside_cards = vec![CardCode::new("01116"), CardCode::new("01117")];
    TestSession::new(state, TEST_REGISTRIES)
}

/// The investigator the round-end window's one act option is offered to, as
/// the prompt names them.
fn offered_to(window: &TestSession) -> InvestigatorId {
    let act: Vec<_> = window
        .prompt()
        .options
        .iter()
        .filter(|o| o.target == Some(OptionTarget::Act))
        .collect();
    let [option] = act[..] else {
        panic!(
            "the round end offers The Barrier's clue spend once: {:?}",
            window.prompt()
        );
    };
    option
        .investigator
        .expect("a reaction window's option names whom it is offered to")
}

fn clues(session: &TestSession, id: InvestigatorId) -> u8 {
    session.state().investigators[&id].clues
}

#[test]
fn with_the_lead_outside_the_hallway_the_hallway_investigator_is_offered_it() {
    // Seat 1 (Daisy) leads from the Attic; seat 2 (Roland) holds the
    // threshold in the Hallway. Seat 2's turn is the round's last.
    let window = barrier_table(
        vec![
            seat(1).as_card("01002").at(ATTIC).clues(1),
            seat(2).as_card("01001").at(HALLWAY).clues(3),
        ],
        &[SEAT_1, SEAT_2],
        SEAT_1,
        SEAT_2,
    )
    .take(&TurnAction::EndTurn);
    assert_eq!(offered_to(&window), SEAT_2);

    let advanced = window.pick(OptionTarget::Act);
    assert_eq!(advanced.state().act_index, 1, "The Barrier advanced");
    assert_eq!(
        (clues(&advanced, SEAT_1), clues(&advanced, SEAT_2)),
        (1, 0),
        "the Hallway investigator spent the threshold; the lead's clue stays",
    );
}

#[test]
fn the_first_hallway_investigator_in_player_order_is_offered_it_and_spends_first() {
    // Seat 2 leads from the Attic, so player order is 2, 3, 1. Seats 3
    // (Daisy) and 1 (Roland) are in the Hallway: seat 3 comes first, though
    // seat 1 is first in seating. Seat 1's turn is the round's last.
    let window = barrier_table(
        vec![
            seat(1).as_card("01001").at(HALLWAY).clues(2),
            seat(2).at(ATTIC),
            seat(3).as_card("01002").at(HALLWAY).clues(2),
        ],
        &[SEAT_1, SEAT_2, SEAT_3],
        SEAT_2,
        SEAT_1,
    )
    .take(&TurnAction::EndTurn);
    assert_eq!(offered_to(&window), SEAT_3);

    let advanced = window.pick(OptionTarget::Act);
    assert_eq!(advanced.state().act_index, 1, "The Barrier advanced");
    assert_eq!(
        (clues(&advanced, SEAT_3), clues(&advanced, SEAT_1)),
        (0, 1),
        "the group spends in player order: seat 3's two, then one of seat 1's",
    );
}

#[test]
fn with_the_lead_in_the_hallway_the_lead_is_offered_it() {
    // Seat 2 (Roland) leads from the Hallway beside seat 1 (Daisy), who comes
    // first in seating but second in player order.
    let window = barrier_table(
        vec![
            seat(1).as_card("01002").at(HALLWAY).clues(2),
            seat(2).as_card("01001").at(HALLWAY).clues(2),
        ],
        &[SEAT_1, SEAT_2],
        SEAT_2,
        SEAT_1,
    )
    .take(&TurnAction::EndTurn);
    assert_eq!(offered_to(&window), SEAT_2);
}
