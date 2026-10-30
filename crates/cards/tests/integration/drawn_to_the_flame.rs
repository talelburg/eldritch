//! End-to-end test for Drawn to the Flame (01064): the encounter card is drawn
//! and resolved in full, and only then are 2 clues discovered at the drawer's
//! location.
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`): *"Draw the top
//! card of the encounter deck. Then, discover 2 clues at your location."*
//!
//! Rulings (<https://arkhamdb.com/card/01064>):
//! - *"You can only 'discover' a clue if there is a clue on your location."*
//! - *"Resolve any keywords and **Revelation** abilities on the encounter card
//!   before discovering clues."*
//! - *"If you play Drawn to the Flame, draw a treachery card that has a move
//!   effect, resolve the move before discovering clues. You discover clues from
//!   the location you end at, if any, rather than the original location at
//!   which you played Drawn to the Flame."*
//!
//! No corpus treachery moves its drawer, so the third ruling has no test here;
//! the discovery reads the drawer's location when it runs, after the drawn
//! card has resolved, which is what the ruling asks.
//!
//! The encounter cards drawn, with their text
//! (`data/arkhamdb-snapshot/pack/core/core_encounter.json`):
//! - Dissonant Voices 01165: *"**Revelation** - Put Dissonant Voices into play
//!   in your threat area."*
//! - Grasping Hands 01162: *"**Revelation** - Test \[agility\] (3). For each
//!   point you fail by, take 1 damage."*
//! - Rotting Remains 01163: *"**Revelation** - Test \[willpower\] (3). For each
//!   point you fail by, take 1 horror."*
//!
//! Psychosis 01099 (`data/arkhamdb-snapshot/pack/core/core.json`) reacts to
//! horror taken: *"**Forced** - After you take 1 or more horror: Take 1 direct
//! damage."*
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, LocationId, Status,
};
use game_core::test_support::{self, seat, Seat, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const DRAWN_TO_THE_FLAME: &str = "01064";
const DISSONANT_VOICES: &str = "01165";
const GRASPING_HANDS: &str = "01162";
const ROTTING_REMAINS: &str = "01163";
const PSYCHOSIS: &str = "01099";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(1);

/// `investigator` (holding Drawn to the Flame, at `HERE` with `clues` clues on
/// it) at their open turn, with `deck` on the encounter deck and `discard` in
/// the encounter discard, top first, and an auto-fail chaos bag.
fn board(investigator: Seat, clues: u8, deck: &[&str], discard: &[&str]) -> TestSession {
    let here = test_support::location(HERE).clues(clues);
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(here)
        .with_seat(investigator.at(HERE).hand([DRAWN_TO_THE_FLAME]))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::AutoFail]))
        .with_encounter_deck(deck.iter().copied())
        .with_encounter_discard(discard.iter().copied())
        .session()
}

fn play(session: TestSession) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    })
}

fn position(events: &[Event], pred: impl Fn(&Event) -> bool) -> usize {
    events
        .iter()
        .position(pred)
        .expect("the event must have been emitted")
}

#[test]
fn the_drawn_card_resolves_before_two_clues_are_discovered() {
    let session = play(board(seat(1), 3, &[DISSONANT_VOICES], &[]));

    session.assert_at_turn_menu(INV);
    let events = session.events();
    let revealed = position(events, |e| {
        matches!(e, Event::CardRevealed { investigator, code, .. }
            if *investigator == INV && code.as_str() == DISSONANT_VOICES)
    });
    let revelation = position(
        events,
        |e| matches!(e, Event::CardEnteredThreatArea { code, .. } if code.as_str() == DISSONANT_VOICES),
    );
    let discovered = position(
        events,
        |e| matches!(e, Event::CluePlaced { investigator, count: 2 } if *investigator == INV),
    );
    assert!(
        revealed < revelation && revelation < discovered,
        "reveal, then Revelation, then the discovery; got {events:#?}",
    );

    let state = session.state();
    assert_eq!(state.investigators[&INV].clues, 2);
    assert_eq!(state.locations[&HERE].clues, 1);
    assert!(state.encounter_deck.is_empty(), "the top card was drawn");
}

/// First ruling: with no clue on the location, nothing is discovered — and the
/// play still completes.
#[test]
fn with_no_clues_at_the_location_the_play_discovers_nothing() {
    let session = play(board(seat(1), 0, &[DISSONANT_VOICES], &[]));

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::CardRevealed { .. });
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    assert_eq!(session.state().investigators[&INV].clues, 0);
}

/// An empty encounter deck is refilled from the encounter discard before the
/// draw — the encounter draw's own rule, not a card-local one.
#[test]
fn an_empty_encounter_deck_is_reshuffled_from_the_discard() {
    let session = play(board(
        seat(1),
        3,
        &[],
        &[DISSONANT_VOICES, DISSONANT_VOICES],
    ));

    session.assert_at_turn_menu(INV);
    let events = session.events();
    let shuffled = position(events, |e| matches!(e, Event::EncounterDeckShuffled));
    let revealed = position(events, |e| matches!(e, Event::CardRevealed { .. }));
    assert!(shuffled < revealed, "the reshuffle precedes the draw");

    let state = session.state();
    assert!(state.encounter_discard.is_empty());
    assert_eq!(state.encounter_deck.len(), 1, "two reshuffled, one drawn");
    assert_eq!(state.investigators[&INV].clues, 2);
}

/// With the encounter deck and its discard both empty there is nothing to
/// draw — a state no rule defines — so the draw is skipped and the discovery
/// still happens.
#[test]
fn with_no_encounter_card_anywhere_the_discovery_still_happens() {
    let session = play(board(seat(1), 3, &[], &[]));

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::CardRevealed { .. });
    assert_eq!(session.state().investigators[&INV].clues, 2);
}

/// A drawn card that defeats its drawer leaves no location to discover at:
/// the play still completes — it is not rolled back — and nothing is
/// discovered.
#[test]
fn a_drawn_card_that_eliminates_the_drawer_discovers_nothing() {
    // Agility 3 vs 3 on an auto-fail: fail by 3, 3 damage on top of 6 of 8.
    let session = play(board(seat(1).harm(6, 0), 3, &[GRASPING_HANDS], &[])).commit(&[]);

    assert!(
        session.rejection().is_none(),
        "the play is not rolled back: {:?}",
        session.rejection()
    );
    assert_event!(
        session.events(),
        Event::InvestigatorEliminated { investigator, .. } if *investigator == INV
    );
    assert_no_event!(session.events(), Event::CluePlaced { .. });
    let state = session.state();
    assert_eq!(state.investigators[&INV].status, Status::Defeated);
    assert_eq!(state.locations[&HERE].clues, 3);
}

/// A consequence raised inside a skill test resolves inside it, so it is not
/// held for the play's post-then discovery. The Official FAQ on Shrivelling
/// 01060's horror (`data/official-faq/Frequently_Asked_Questions.md`): *"If you
/// had any reactions to taking that horror (such as Agnes Baker’s ability), you
/// would trigger it then, before resolving the rest of the attack."* Rotting
/// Remains' failed test deals its horror, Psychosis deals its damage, and only
/// then are 2 clues discovered.
#[test]
fn psychosis_reacts_to_a_drawn_tests_horror_before_two_clues_are_discovered() {
    let session = play(board(
        seat(1).threat_area([PSYCHOSIS]),
        3,
        &[ROTTING_REMAINS],
        &[],
    ))
    .commit(&[]);

    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 3 } if *investigator == INV,
        Event::DamagePlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV,
        Event::CluePlaced { investigator, count: 2 } if *investigator == INV,
    );
}
