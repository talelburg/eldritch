//! Burglary 01045 end-to-end against the real corpus.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01045):
//!
//! > [action] Exhaust Burglary: <b>Investigate.</b> If you succeed, instead of
//! > discovering clues, gain 3 resources.
//!
//! Ruling (<https://arkhamdb.com/card/01045>):
//!
//! > You can use Burglary even if there are no clues on your location.
//!
//! Deduction 01039's ruling (<https://arkhamdb.com/card/01039>) names Burglary:
//! *"it modifies the number of clues that you would find, it does not add an
//! extra effect on top of any other effects (e.g. does not stack with
//! Burglary)."*
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, GameState, GameStateBuilder,
    InvestigatorId, LocationId, SkillKind,
};
use game_core::test_support::{self, seat, BoardLookup as _, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

const BURGLARY: &str = "01045";
const DR_MILAN: &str = "01033";
const DEDUCTION: &str = "01039";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The burglar: Burglary in play, no resources, 3 actions, at the Study with
/// intellect `intellect`.
fn burglar(intellect: i8) -> Seat {
    seat(1)
        .at(HERE)
        .skill(SkillKind::Intellect, intellect)
        .in_play([BURGLARY])
        .resources(0)
        .actions(3)
}

/// `investigator`'s turn at a shroud-2 Study holding `clues` clues, with a
/// `Numeric(0)` bag.
fn board(investigator: Seat, clues: u8) -> TestSession {
    // shroud 2
    let study = test_support::location(HERE).clues(clues);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_location(study)
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .session()
}

/// Burglary's `[action]` ability.
fn activation(state: &GameState) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, BURGLARY)),
        address: AbilityAddress::Printed(0),
    }
}

/// Activate Burglary off the menu and commit `commits` to its investigation.
fn burgle(session: TestSession, commits: &[&str]) -> TestSession {
    let activation = activation(session.state());
    session.take(&activation).commit(commits)
}

fn resources(state: &GameState) -> u8 {
    state.investigators[&INV].resources
}

#[test]
fn success_gains_three_resources_instead_of_discovering_a_clue() {
    // Intellect 3 vs shroud 2: success.
    let session = burgle(board(burglar(3), 2), &[]);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_eq!(resources(session.state()), 3, "3 resources gained");
    assert_eq!(
        session.state().investigators[&INV].clues,
        0,
        "no clue discovered"
    );
    assert_eq!(
        session.state().locations[&HERE].clues,
        2,
        "the location's clues are unchanged"
    );
    assert_no_event!(session.events(), Event::CluePlaced { .. });
}

#[test]
fn failure_gains_nothing() {
    // Intellect 1 vs shroud 2: failure.
    let session = burgle(board(burglar(1), 2), &[]);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_eq!(resources(session.state()), 0);
    assert_eq!(session.state().locations[&HERE].clues, 2);
}

#[test]
fn usable_at_a_location_with_no_clues() {
    // `take` picks the activation off the turn menu, so this also proves it is
    // offered at a clueless location.
    let session = burgle(board(burglar(3), 0), &[]);
    session.assert_at_turn_menu(INV);
    assert_eq!(resources(session.state()), 3);
}

#[test]
fn burglary_exhausts_and_costs_an_action() {
    let session = burgle(board(burglar(3), 2), &[]);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].actions_remaining, 2);
    let burglary = session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.code.as_str() == BURGLARY)
        .expect("Burglary stays in play");
    assert!(burglary.exhausted, "Burglary exhausts");
}

#[test]
fn a_successfully_investigate_reaction_still_fires() {
    // Dr. Milan Christopher 01033: "You get +1 [intellect]. [reaction] After
    // you successfully investigate: Gain 1 resource." Intellect 1 + 1 = 2 vs
    // shroud 2: success.
    let session = board(burglar(1).in_play([DR_MILAN]), 2);
    let dr_milan = session.state().card(INV, DR_MILAN);
    let window = burgle(session, &[]);
    assert_eq!(
        window.offered_at(&dr_milan),
        1,
        "Burglary's investigation is a successful investigation, got {:?}",
        window.prompt(),
    );
    let resumed = window.pick(dr_milan);
    resumed.assert_at_turn_menu(INV);
    assert_eq!(
        resources(resumed.state()),
        1 + 3,
        "Dr. Milan's 1 and Burglary's 3"
    );
    assert_eq!(
        resumed.state().locations[&HERE].clues,
        2,
        "still no clue discovered"
    );
}

#[test]
fn deduction_does_not_stack_with_burglary() {
    // Deduction 01039 ([intellect] icon): "If this skill test is successful
    // while investigating a location, discover 1 additional clue at that
    // location." Its ruling: it does not stack with Burglary.
    let session = burgle(board(burglar(2).hand([DEDUCTION]), 2), &[DEDUCTION]);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_eq!(resources(session.state()), 3);
    assert_eq!(
        session.state().investigators[&INV].clues,
        0,
        "no clue discovered"
    );
    assert_eq!(session.state().locations[&HERE].clues, 2);
}
