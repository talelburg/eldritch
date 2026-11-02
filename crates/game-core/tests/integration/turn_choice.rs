//! The lead chooses who takes each Investigation turn while two or more
//! investigators are still waiting for one.
//!
//! Appendix II 2.2: *"The investigators may take their turns in any order. The
//! investigators choose among themselves who (among the investigators) will
//! take this turn, and making this choice begins that investigator's turn."*
//! `glossary/Lead_Investigator.md` makes the lead *"the final arbiter in
//! choosing among those options"*, so the lead answers the choice (ADR 0019).
//!
//! Each test stages the Investigation phase just after step 2.1's forced
//! abilities, so the post-2.1 player window (empty here) closes and step 2.2
//! runs.

use game_core::action::InputResponse;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Responder;
use game_core::event::Event;
use game_core::state::{GameStateBuilder, InvestigatorId, Phase, Status};
use game_core::test_support::{seat, Seat, TestSession, TEST_REGISTRIES};
use game_core::{assert_event, assert_no_event};

const FIRST_SEAT: InvestigatorId = InvestigatorId(1);
const SECOND_SEAT: InvestigatorId = InvestigatorId(2);
const THIRD_SEAT: InvestigatorId = InvestigatorId(3);

/// Two seats `[1, 2]`, seat 2 the lead.
fn two_seats_led_by_the_second() -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_two_seats_led_by_the_second(seat(1), seat(2))
}

/// Three seats `[1, 2, 3]`, seat 1 the lead, built from `seats`.
fn three_seats(seats: [Seat; 3]) -> GameStateBuilder {
    let [one, two, three] = seats;
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(one)
        .with_seat(two)
        .with_seat(three)
        .with_seating([FIRST_SEAT, SECOND_SEAT, THIRD_SEAT])
}

/// The investigators the current prompt offers, by their investigator card.
fn offered(session: &TestSession) -> Vec<InvestigatorId> {
    let state = session.state();
    session
        .prompt()
        .options
        .iter()
        .map(|option| {
            *state
                .investigators
                .iter()
                .find(|(_, inv)| option.target == Some(inv.card_anchor()))
                .unwrap_or_else(|| panic!("option {option:?} is not on an investigator card"))
                .0
        })
        .collect()
}

#[test]
fn the_lead_is_asked_who_takes_the_first_turn_and_their_pick_opens_it() {
    let session = two_seats_led_by_the_second()
        .beginning_investigation_phase()
        .session();

    assert_eq!(
        session.prompt().prompt,
        "The lead investigator chooses who takes the next turn."
    );
    assert_eq!(session.prompt().responder, Responder::Lead(SECOND_SEAT));
    assert_eq!(offered(&session), [SECOND_SEAT, FIRST_SEAT], "player order");
    let session = session.choose_turn(FIRST_SEAT);
    session.assert_at_turn_menu(FIRST_SEAT);
}

#[test]
fn the_last_investigator_waiting_takes_their_turn_unasked() {
    let session = two_seats_led_by_the_second()
        .beginning_investigation_phase()
        .session()
        .choose_turn(FIRST_SEAT)
        .take(&TurnAction::EndTurn);
    session.assert_at_turn_menu(SECOND_SEAT);
    assert_no_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Investigation
        }
    );

    let session = session.take(&TurnAction::EndTurn);
    assert_event!(
        session.events(),
        Event::PhaseEnded {
            phase: Phase::Investigation
        }
    );
}

#[test]
fn the_second_choice_offers_only_the_investigators_still_waiting() {
    let session = three_seats([seat(1), seat(2), seat(3)])
        .beginning_investigation_phase()
        .session();
    assert_eq!(offered(&session), [FIRST_SEAT, SECOND_SEAT, THIRD_SEAT]);

    let session = session.choose_turn(SECOND_SEAT).take(&TurnAction::EndTurn);
    assert_eq!(session.prompt().responder, Responder::Lead(FIRST_SEAT));
    assert_eq!(offered(&session), [FIRST_SEAT, THIRD_SEAT]);
    session
        .choose_turn(THIRD_SEAT)
        .assert_at_turn_menu(THIRD_SEAT);
}

#[test]
fn an_answer_from_the_investigator_who_is_not_the_lead_is_rejected() {
    let session = two_seats_led_by_the_second()
        .beginning_investigation_phase()
        .session();
    let anchor = session.state().investigators[&FIRST_SEAT].card_anchor();
    let option = session
        .prompt()
        .options
        .iter()
        .find(|option| option.target == Some(anchor.clone()))
        .expect("seat 1 is offered")
        .id;

    let _ = session.assert_rejects(|s| s.answer_as(FIRST_SEAT, InputResponse::PickSingle(option)));
}

#[test]
fn a_solo_game_never_sees_the_choice() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .with_seating([FIRST_SEAT])
        .beginning_investigation_phase()
        .session();
    session.assert_at_turn_menu(FIRST_SEAT);
}

#[test]
fn an_investigator_eliminated_before_their_turn_is_not_offered() {
    let session = three_seats([seat(1), seat(2).status(Status::Defeated), seat(3)])
        .beginning_investigation_phase()
        .session();
    assert_eq!(offered(&session), [FIRST_SEAT, THIRD_SEAT]);
}

#[test]
fn nobody_is_the_active_investigator_while_the_choice_is_pending() {
    let session = two_seats_led_by_the_second()
        .beginning_investigation_phase()
        .session();
    assert_eq!(session.state().active_investigator, None);

    let session = three_seats([seat(1), seat(2), seat(3)])
        .beginning_investigation_phase()
        .session()
        .choose_turn(FIRST_SEAT)
        .take(&TurnAction::EndTurn);
    assert_eq!(offered(&session), [SECOND_SEAT, THIRD_SEAT]);
    assert_eq!(
        session.state().active_investigator,
        None,
        "the choice after a turn ends has nobody active either"
    );
}

#[test]
#[should_panic(expected = "not a turn choice")]
fn choose_turn_panics_off_a_turn_choice() {
    // A solo game rests at the turn menu, not at a turn choice.
    let _ = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1))
        .with_seating([FIRST_SEAT])
        .beginning_investigation_phase()
        .session()
        .choose_turn(FIRST_SEAT);
}
