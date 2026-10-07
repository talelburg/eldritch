//! Reactions and elimination (#959). `glossary/Elimination.md`: *"The only
//! manner in which eliminated investigators interact with the game is when
//! establishing "per investigator" values"* — so an eliminated investigator is
//! never offered a reaction, and the initiation gate refuses a controller who
//! is not Active (ADR 0017). The act's and agenda's reactions bind the lead
//! proxy, the first Active investigator in `turn_order` (GLOSSARY "Lead
//! investigator"), so they survive the first seat's elimination.
//!
//! Integration test so it can install `cards::REGISTRY` in its own process.

use cards::REGISTRY;
use game_core::engine::TimingEvent;
use game_core::state::{
    Act, CardCode, EnemyId, GameState, GameStateBuilder, InvestigatorId, Location, LocationId,
    ResolutionCandidate, Status,
};
use game_core::test_support::{self, TestSession};

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

const SEAT_1: InvestigatorId = InvestigatorId(1);
const SEAT_2: InvestigatorId = InvestigatorId(2);
const STUDY: LocationId = LocationId(10);
const HALLWAY: LocationId = LocationId(2);

/// The candidates the window `event` opens offers, empty when none opened.
fn offered(state: GameState, event: TimingEvent) -> Vec<ResolutionCandidate> {
    TestSession::new(state)
        .fire_at(event)
        .state()
        .top_window()
        .and_then(|w| w.pending_candidates())
        .cloned()
        .unwrap_or_default()
}

/// Two seats; seat 1 is Roland Banks (01001), seated, at a Study with a clue
/// on it, and has `status`. 01001: *"[reaction] After you defeat an enemy:
/// Discover 1 clue at your location. (Limit once per round.)"*
fn roland_table(status: Status) -> GameState {
    let mut roland = test_support::test_investigator(1);
    roland.investigator_card.code = CardCode::new("01001");
    roland.current_location = Some(STUDY);
    roland.status = status;
    let mut other = test_support::test_investigator(2);
    other.current_location = Some(STUDY);
    let mut study = test_support::test_location(10, "Study");
    study.clues = 1;
    GameStateBuilder::new()
        .with_investigator(roland)
        .with_investigator(other)
        .with_location(study)
        .with_turn_order([SEAT_1, SEAT_2])
        .build()
}

fn roland_defeats_an_enemy() -> TimingEvent {
    TimingEvent::EnemyDefeated {
        enemy: EnemyId(100),
        by: Some(SEAT_1),
        code: CardCode::new("01160"),
    }
}

/// Control for the test below: the same table with Roland Active offers his
/// reaction, so only his status separates the two.
#[test]
fn an_active_roland_is_offered_his_reaction() {
    let offered = offered(roland_table(Status::Active), roland_defeats_an_enemy());
    assert_eq!(
        offered
            .iter()
            .map(|c| (c.code.clone(), c.controller))
            .collect::<Vec<_>>(),
        vec![(CardCode::new("01001"), SEAT_1)],
    );
}

/// Elimination leaves the investigator card behind (it cannot be drained,
/// #448), so a defeated Roland still holds his printed reaction; the gate's
/// status check is what keeps it from being offered.
#[test]
fn an_eliminated_investigator_is_offered_no_reaction() {
    let offered = offered(roland_table(Status::Defeated), roland_defeats_an_enemy());
    assert_eq!(offered, Vec::new());
}

/// The Barrier (01109) is the current act, and seat 2 stands in the Hallway
/// (01112) holding the act's clue threshold. Seat 1 has been eliminated.
/// 01109: *"Objective - When the round ends, investigators in the hallway may,
/// as a group, spend the requisite number of clues to advance."*
fn barrier_table_with_seat_1_eliminated() -> GameState {
    let mut eliminated = test_support::test_investigator(1);
    eliminated.status = Status::Defeated;
    eliminated.current_location = None;
    let mut survivor = test_support::test_investigator(2);
    survivor.current_location = Some(HALLWAY);
    survivor.clues = 3;
    let mut state = GameStateBuilder::new()
        .with_investigator(eliminated)
        .with_investigator(survivor)
        .with_location(Location::new(
            HALLWAY,
            CardCode::new("01112"),
            "Hallway",
            1,
            0,
        ))
        .with_turn_order([SEAT_1, SEAT_2])
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new("01109"),
        clue_threshold: 3,
    }];
    state.act_index = 0;
    state
}

#[test]
fn an_act_reaction_survives_the_first_seat_being_eliminated() {
    let offered = offered(
        barrier_table_with_seat_1_eliminated(),
        TimingEvent::RoundEnded,
    );
    assert_eq!(
        offered
            .iter()
            .map(|c| (c.code.clone(), c.controller))
            .collect::<Vec<_>>(),
        vec![(CardCode::new("01109"), SEAT_2)],
        "the act's reaction is controlled by the first Active investigator",
    );
}
