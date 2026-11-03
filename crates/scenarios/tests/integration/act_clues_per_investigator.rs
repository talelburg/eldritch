//! An act's printed clue value scales by the investigators who started the
//! scenario: The Gathering seated with two investigators asks 4 clues of
//! Trapped (01108, "2 per investigator") and 6 of The Barrier (01109, "3 per
//! investigator"). The fixed-value case, Finding A Way Inside (02122), is in
//! `cards`' `act_advancement` tests. With a surplus over that value, the
//! investigator who did not start the advance chooses their share and the one
//! who did covers the rest.
//!
//! Glossary, Per Investigator: *"When the \[per_investigator\] symbol appears
//! after a value, that value is multiplied by the number of investigators who
//! started the scenario."*

use game_core::action::RosterEntry;
use game_core::assert_event;
use game_core::engine;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{OptionTarget, Responder};
use game_core::event::Event;
use game_core::state::{CardCode, GameState, InvestigatorId};
use game_core::test_support::TestSession;
use scenarios::the_gathering;

const ROLAND: InvestigatorId = InvestigatorId(1);
const DAISY: InvestigatorId = InvestigatorId(2);
/// The Hallway, whose investigators spend clues for The Barrier.
const HALLWAY: &str = "01112";

/// The Gathering seated with Roland (01001) and Daisy (01002), both keeping
/// their (empty) opening hands, and the lead choosing Roland to go first:
/// Roland's turn menu, under Act 1.
fn seated_pair() -> GameState {
    let roster: Vec<_> = ["01001", "01002"]
        .into_iter()
        .map(|code| RosterEntry {
            investigator: CardCode::new(code),
            deck: vec![],
            random_basic_weaknesses: 0,
        })
        .collect();
    let opened = engine::seat_and_open(
        the_gathering::setup(),
        &roster,
        0,
        scenarios::TEST_REGISTRIES,
    );
    TestSession::new(opened.state, scenarios::TEST_REGISTRIES)
        .choose_from_hand(&[])
        .choose_from_hand(&[])
        .choose_turn(ROLAND)
        .finish()
        .state
}

fn give_clues(state: &mut GameState, investigator: InvestigatorId, clues: u8) {
    state
        .investigators
        .get_mut(&investigator)
        .expect("seated")
        .clues = clues;
}

fn advance_offered(state: &GameState) -> bool {
    enumerate::legal_actions(state, scenarios::TEST_REGISTRIES.cards)
        .iter()
        .any(|a| matches!(a, TurnAction::AdvanceAct { .. }))
}

#[test]
fn trapped_asks_four_clues_of_two_investigators() {
    let mut state = seated_pair();
    assert_eq!(state.act_deck[state.act_index].code.as_str(), "01108");

    give_clues(&mut state, ROLAND, 2);
    give_clues(&mut state, DAISY, 1);
    assert!(!advance_offered(&state), "3 clues < 2 × 2 investigators");
    TestSession::new(state.clone(), scenarios::TEST_REGISTRIES)
        .submit_off_menu(&TurnAction::AdvanceAct {
            investigator: ROLAND,
        })
        .expect_rejected();

    give_clues(&mut state, DAISY, 2);
    assert!(advance_offered(&state), "4 clues = 2 × 2 investigators");
    let session =
        TestSession::new(state, scenarios::TEST_REGISTRIES).take(&TurnAction::AdvanceAct {
            investigator: ROLAND,
        });
    assert_event!(session.events(), Event::ActAdvanced { .. });
    let state = session.state();
    assert_eq!(
        (
            state.investigators[&ROLAND].clues,
            state.investigators[&DAISY].clues
        ),
        (0, 0),
        "all 4 clues are spent",
    );
}

/// Trapped 01108 asks 4 of Roland's 3 and Daisy's 3. Roland takes the advance,
/// so Daisy is asked first and offered 1 to 3, and Roland covers the rest.
/// `glossary/Act_Deck_and_Agenda_Deck.md`: *"Any or all investigators may
/// contribute any number of clues towards the total number of clues required to
/// advance the act."*
#[test]
fn daisy_chooses_her_share_of_trapped_and_roland_covers_the_rest() {
    let mut state = seated_pair();
    give_clues(&mut state, ROLAND, 3);
    give_clues(&mut state, DAISY, 3);
    let session =
        TestSession::new(state, scenarios::TEST_REGISTRIES).take(&TurnAction::AdvanceAct {
            investigator: ROLAND,
        });
    assert_eq!(session.prompt().responder, Responder::Investigator(DAISY));
    assert_eq!(
        labels(&session),
        [
            "Contribute 1 clue",
            "Contribute 2 clues",
            "Contribute 3 clues"
        ]
    );

    // Daisy puts in 3, so Roland keeps 2 of his.
    let session = session.pick_nth(2);
    assert_event!(session.events(), Event::ActAdvanced { .. });
    assert_eq!(clues(&session, ROLAND, DAISY), (2, 0));
}

/// The Barrier 01109: *"When the round ends, investigators in the hallway may,
/// as a group, spend the requisite number of clues to advance."* Roland and
/// Daisy stand in the Hallway with 4 clues each against its 6. The round-end
/// advance is offered to Roland, the first of them in player order, so Daisy is
/// asked first and offered 2 to 4, and Roland covers the rest.
#[test]
fn daisy_chooses_her_share_of_the_barrier_at_round_end() {
    // Trapped's reverse puts the Hallway into play and places both
    // investigators in it.
    let mut state = seated_pair();
    give_clues(&mut state, ROLAND, 4);
    let mut state = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .take(&TurnAction::AdvanceAct {
            investigator: ROLAND,
        })
        .state()
        .clone();
    assert_eq!(state.act_deck[state.act_index].code.as_str(), "01109");
    give_clues(&mut state, ROLAND, 4);
    give_clues(&mut state, DAISY, 4);

    let session = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .take(&TurnAction::EndTurn)
        .take(&TurnAction::EndTurn)
        .pick_for(ROLAND, OptionTarget::Act);
    assert_eq!(session.prompt().responder, Responder::Investigator(DAISY));
    assert_eq!(
        labels(&session),
        [
            "Contribute 2 clues",
            "Contribute 3 clues",
            "Contribute 4 clues"
        ]
    );

    // Daisy puts in 2, so Roland gives 4.
    let session = session.pick_nth(0);
    assert_ne!(
        session.state().act_deck[session.state().act_index]
            .code
            .as_str(),
        "01109",
        "The Barrier advanced"
    );
    assert_eq!(clues(&session, ROLAND, DAISY), (0, 2));
}

fn labels(session: &TestSession) -> Vec<&str> {
    session
        .prompt()
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect()
}

fn clues(session: &TestSession, a: InvestigatorId, b: InvestigatorId) -> (u8, u8) {
    let investigators = &session.state().investigators;
    (investigators[&a].clues, investigators[&b].clues)
}

#[test]
fn the_barrier_asks_six_clues_of_two_investigators_in_the_hallway() {
    // Trapped's reverse puts the Hallway into play and places both
    // investigators in it.
    let mut state = seated_pair();
    give_clues(&mut state, ROLAND, 4);
    let mut state = TestSession::new(state, scenarios::TEST_REGISTRIES)
        .take(&TurnAction::AdvanceAct {
            investigator: ROLAND,
        })
        .state()
        .clone();
    assert_eq!(state.act_deck[state.act_index].code.as_str(), "01109");

    give_clues(&mut state, ROLAND, 3);
    give_clues(&mut state, DAISY, 2);
    assert!(
        !engine::round_end_advance_affordable(&state, HALLWAY),
        "5 clues < 3 × 2 investigators"
    );
    give_clues(&mut state, DAISY, 3);
    assert!(
        engine::round_end_advance_affordable(&state, HALLWAY),
        "6 clues = 3 × 2 investigators"
    );
}
