//! "Look what I found!" 01079 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01079):
//!
//! > Fast. Play after you fail a skill test by 2 or less while investigating.
//! > Discover 2 clues in your location.
//!
//! Ruling (<https://arkhamdb.com/card/01079>):
//!
//! > You can only 'discover' a clue if there is a clue on your location.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{self, seat, TestSession};

const LOOK_WHAT_I_FOUND: &str = "01079";

const INV: InvestigatorId = InvestigatorId(1);
/// A shroud-4 Study.
const HERE: LocationId = LocationId(10);

/// Investigator 1 at a shroud-4 Study holding `clues` clues, with intellect
/// `intellect`, 5 resources and "Look what I found!" in hand, on their turn.
fn board(intellect: i8, clues: u8) -> TestSession {
    let study = test_support::location(HERE).shroud(4).clues(clues);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(
            seat(1)
                .at(HERE)
                .skill(SkillKind::Intellect, intellect)
                .resources(5)
                .hand([LOOK_WHAT_I_FOUND]),
        )
        .with_location(study)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .open_turn(INV)
        .session()
}

fn in_hand() -> OptionTarget {
    OptionTarget::HandCardByCode {
        investigator: INV,
        code: CardCode::new(LOOK_WHAT_I_FOUND),
    }
}

fn investigate(session: TestSession) -> TestSession {
    session
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[])
}

fn still_in_hand(session: &TestSession) -> bool {
    session.state().investigators[&INV]
        .hand
        .contains(&CardCode::new(LOOK_WHAT_I_FOUND))
}

#[test]
fn failing_an_investigation_by_two_offers_it_and_discovers_two_clues() {
    // Intellect 2 against shroud 4: failed by 2.
    let window = investigate(board(2, 3));
    assert_eq!(window.offered_at(&in_hand()), 1, "{:?}", window.prompt());

    let done = window.pick(in_hand());
    done.assert_at_turn_menu(INV);
    assert_eq!(done.state().investigators[&INV].clues, 2);
    assert_eq!(done.state().locations[&HERE].clues, 1);
    assert_eq!(done.state().investigators[&INV].resources, 3, "cost 2 paid");
}

#[test]
fn failing_an_investigation_by_three_does_not_offer_it() {
    // Intellect 1 against shroud 4: failed by 3.
    let done = investigate(board(1, 3));
    done.assert_at_turn_menu(INV);
    assert!(still_in_hand(&done));
    assert_eq!(done.state().locations[&HERE].clues, 3);
}

#[test]
fn failing_a_test_that_is_not_an_investigation_does_not_offer_it() {
    // A plain intellect test, intellect 2 against 4: failed by 2, but not
    // while investigating.
    let done = board(2, 3)
        .test_skill(INV, SkillKind::Intellect, 4)
        .commit(&[]);
    done.assert_at_turn_menu(INV);
    assert!(still_in_hand(&done));
}

#[test]
fn it_discovers_only_the_clues_the_location_has() {
    let done = investigate(board(2, 1)).pick(in_hand());
    done.assert_at_turn_menu(INV);
    assert_eq!(done.state().investigators[&INV].clues, 1);
    assert_eq!(done.state().locations[&HERE].clues, 0);
}
