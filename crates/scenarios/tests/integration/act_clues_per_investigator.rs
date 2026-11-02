//! An act's printed clue value scales by the investigators who started the
//! scenario: The Gathering seated with two investigators asks 4 clues of
//! Trapped (01108, "2 per investigator") and 6 of The Barrier (01109, "3 per
//! investigator"). The fixed-value case, Finding A Way Inside (02122), is in
//! `cards`' `act_advancement` tests.
//!
//! Glossary, Per Investigator: *"When the \[per_investigator\] symbol appears
//! after a value, that value is multiplied by the number of investigators who
//! started the scenario."*

use game_core::action::RosterEntry;
use game_core::assert_event;
use game_core::engine;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::event::Event;
use game_core::state::{CardCode, GameState, InvestigatorId};
use game_core::test_support::TestSession;
use scenarios::the_gathering;

const ROLAND: InvestigatorId = InvestigatorId(1);
const DAISY: InvestigatorId = InvestigatorId(2);
/// The Hallway, whose investigators spend clues for The Barrier.
const HALLWAY: &str = "01112";

/// The Gathering seated with Roland (01001) and Daisy (01002), both keeping
/// their (empty) opening hands: Roland's turn menu, under Act 1.
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
