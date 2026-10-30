//! Scrying 01061 end-to-end against the real card registry:
//!
//! ```text
//! Uses (3 charges).
//! [action] Exhaust Scrying and spend 1 charge: Look at the top 3 cards of any
//!   investigator's deck or the encounter deck. Return them to the top of that
//!   deck in any order.
//! ```
//!
//! No rulings: 01061 is listed in `data/arkhamdb-faq/no-rulings.txt`.
//!
//! The controller first decides which deck (a decision printed on the card:
//! "Your deck" or "The encounter deck"; solo, "any investigator's deck" is
//! your own), then rebuilds the top of it one card at a time: each pick puts
//! one of the remaining looked-at cards next in line from the top, and the
//! last card needs no pick.
//!
//! Each per-card pick is sent raw: its options are cards **in a deck**, with no
//! board anchor and neither of ADR 0015's natures, so no session step can name
//! one and the pick is positional (Old Book of Lore's test does the same).
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::card_data::UseKind;
use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionId;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, GameStateBuilder, InvestigatorId,
    LocationId,
};
use game_core::test_support::{self, seat, uses, Seat, SeatLookup as _, TestSession};

const SCRYING: &str = "01061";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Player cards for the deck, by name so a prompt's labels read as names.
const EMERGENCY_CACHE: &str = "01088";
const GUTS: &str = "01089";
const PERCEPTION: &str = "01090";
const OVERPOWER: &str = "01091";

/// Encounter cards for the encounter deck.
const ANCIENT_EVILS: &str = "01166";
const CRYPT_CHILL: &str = "01167";
const OBSCURING_FOG: &str = "01168";
const ROTTING_REMAINS: &str = "01163";

/// The deck decision's branches, in printed order.
const YOUR_DECK: usize = 0;
const ENCOUNTER_DECK: usize = 1;

/// Investigator 1 at `HERE` with a fully charged Scrying in play and `deck`
/// as their deck, top first.
fn seer(deck: &[&str]) -> Seat {
    seat(1)
        .at(HERE)
        .in_play([(SCRYING, uses(UseKind::Charges, 3))])
        .deck(deck.iter().copied())
}

/// `investigator`'s turn, alone at `HERE`, with `encounter` as the encounter
/// deck, top first.
fn board(investigator: Seat, encounter: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
        .session()
        .edit_state(|state| {
            state
                .encounter_deck
                .extend(encounter.iter().map(|c| CardCode::new(*c)));
        })
}

fn scrying(session: &TestSession) -> CardInstanceId {
    session.state().in_play(INV, SCRYING)
}

fn activate(scrying: CardInstanceId) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(scrying),
        address: AbilityAddress::Printed(0),
    }
}

/// Put the `position`th of the cards still to be placed next from the top.
fn place(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

fn labels(session: &TestSession) -> Vec<&str> {
    session
        .prompt()
        .options
        .iter()
        .map(|o| o.label.as_str())
        .collect()
}

fn codes(deck: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<CardCode> {
    deck.into_iter()
        .map(|c| CardCode::new(c.as_ref()))
        .collect()
}

#[test]
fn reordering_your_deck_puts_the_chosen_order_on_top() {
    let session = board(seer(&[EMERGENCY_CACHE, GUTS, PERCEPTION, OVERPOWER]), &[]);
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(YOUR_DECK);
    let card = s.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == scrying)
        .expect("Scrying in play");
    assert!(card.exhausted, "exhausting Scrying is a cost");
    assert_eq!(
        card.uses.get(&UseKind::Charges).copied(),
        Some(2),
        "spending 1 charge is a cost",
    );
    assert_eq!(
        labels(&s),
        ["Emergency Cache", "Guts", "Perception"],
        "the top 3 cards are looked at, and only those",
    );

    // Perception goes on top, then Emergency Cache; Guts is left third.
    let s = s.apply(place(2));
    assert_eq!(labels(&s), ["Emergency Cache", "Guts"]);
    let s = s.apply(place(0));

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes([PERCEPTION, EMERGENCY_CACHE, GUTS, OVERPOWER]),
        "the chosen order is on top; the card below the top 3 is untouched",
    );
}

#[test]
fn reordering_the_encounter_deck_works_the_same() {
    let session = board(
        seer(&[EMERGENCY_CACHE]),
        &[ANCIENT_EVILS, CRYPT_CHILL, OBSCURING_FOG, ROTTING_REMAINS],
    );
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(ENCOUNTER_DECK);
    assert_eq!(
        labels(&s),
        ["Ancient Evils", "Crypt Chill", "Obscuring Fog"]
    );

    // Crypt Chill on top, then Obscuring Fog; Ancient Evils is left third.
    let s = s.apply(place(1));
    assert_eq!(labels(&s), ["Ancient Evils", "Obscuring Fog"]);
    let s = s.apply(place(1));

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().encounter_deck.iter().cloned().collect::<Vec<_>>(),
        codes([CRYPT_CHILL, OBSCURING_FOG, ANCIENT_EVILS, ROTTING_REMAINS]),
    );
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes([EMERGENCY_CACHE]),
        "your own deck is untouched",
    );
}

#[test]
fn a_deck_of_two_cards_takes_one_pick() {
    let session = board(seer(&[GUTS, PERCEPTION]), &[]);
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(YOUR_DECK);
    assert_eq!(labels(&s), ["Guts", "Perception"]);
    let s = s.apply(place(1));

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes([PERCEPTION, GUTS])
    );
}

#[test]
fn a_deck_of_one_card_takes_no_pick() {
    let session = board(seer(&[GUTS]), &[]);
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(YOUR_DECK);

    s.assert_at_turn_menu(INV);
    assert_eq!(s.state().investigators[&INV].deck, codes([GUTS]));
    let card = s.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == scrying)
        .expect("Scrying in play");
    assert!(card.exhausted, "the costs are paid all the same");
    assert_eq!(card.uses.get(&UseKind::Charges).copied(), Some(2));
}
