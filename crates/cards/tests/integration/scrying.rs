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
//! "An investigator's deck" or "The encounter deck"). On the investigator-deck
//! branch they then choose the investigator, a selection anchored on each
//! investigator card, which binds without a prompt when only one investigator
//! is left in the game. Then they rebuild the top of the deck one card at a
//! time: each pick puts one of the remaining looked-at cards next in line from
//! the top, and the last card needs no pick.
//!
//! Each per-card pick names its card by its position in the deck, the anchor
//! the engine attaches to a card it offers from outside play, and the prompt
//! names the looked-at cards as its area (ADR 0011).
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::card_data::UseKind;
use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, PromptNature, Responder};
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, DeckIndex, GameStateBuilder,
    InvestigatorId, LocationId, Status,
};
use game_core::test_support::{location, seat, uses, BoardLookup as _, Seat, TestSession};

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
const INVESTIGATOR_DECK: usize = 0;
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
        .with_location(location(HERE))
        .with_seat(investigator)
        .open_turn(INV)
        .with_encounter_deck(encounter.iter().copied())
        .session()
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

/// The card `index` cards below the top of `investigator`'s deck: what a
/// pick names to put that card next.
fn deck_card(investigator: InvestigatorId, index: u8) -> OptionTarget {
    OptionTarget::DeckCard {
        investigator,
        index: DeckIndex(index),
    }
}

/// The card `index` cards below the top of the encounter deck.
fn encounter_card(index: u8) -> OptionTarget {
    OptionTarget::EncounterDeckCard {
        index: DeckIndex(index),
    }
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

    let s = session.take(&activate(scrying)).pick_nth(INVESTIGATOR_DECK);
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
        s.offered_labels(),
        ["Emergency Cache", "Guts", "Perception"],
        "the top 3 cards are looked at, and only those",
    );
    assert_eq!(
        s.offered_targets(),
        [0, 1, 2].map(|i| Some(deck_card(INV, i))),
        "each card is anchored by its position in the deck",
    );
    assert_eq!(
        s.prompt().target,
        Some(OptionTarget::LookedAtTopOfDeck {
            investigator: INV,
            count: 3,
        }),
        "the prompt names the cards looked at, which are not searched",
    );
    assert_eq!(s.prompt().responder, Responder::Investigator(INV));

    // Perception goes on top, then Emergency Cache; Guts is left third.
    let s = s.pick(deck_card(INV, 2));
    assert_eq!(s.offered_labels(), ["Emergency Cache", "Guts"]);
    assert_eq!(
        s.offered_targets(),
        [1, 2].map(|i| Some(deck_card(INV, i))),
        "the placed card is not offered again",
    );
    assert_eq!(
        s.prompt().target,
        Some(OptionTarget::LookedAtTopOfDeck {
            investigator: INV,
            count: 3,
        }),
        "the area is still every card looked at, the placed one included",
    );
    let s = s.pick(deck_card(INV, 1));

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
        s.offered_labels(),
        ["Ancient Evils", "Crypt Chill", "Obscuring Fog"]
    );
    assert_eq!(
        s.offered_targets(),
        [0, 1, 2].map(|i| Some(encounter_card(i))),
    );
    assert_eq!(
        s.prompt().target,
        Some(OptionTarget::LookedAtTopOfEncounterDeck { count: 3 }),
    );
    assert_eq!(s.prompt().responder, Responder::Investigator(INV));

    // Crypt Chill on top, then Obscuring Fog; Ancient Evils is left third.
    let s = s.pick(encounter_card(1));
    assert_eq!(s.offered_labels(), ["Ancient Evils", "Obscuring Fog"]);
    let s = s.pick(encounter_card(2));

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

    let s = session.take(&activate(scrying)).pick_nth(INVESTIGATOR_DECK);
    assert_eq!(s.offered_labels(), ["Guts", "Perception"]);
    assert_eq!(
        s.prompt().target,
        Some(OptionTarget::LookedAtTopOfDeck {
            investigator: INV,
            count: 2,
        }),
        "the area is the 2 cards the deck holds, not the 3 printed",
    );
    let s = s.pick(deck_card(INV, 1));

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes([PERCEPTION, GUTS])
    );
}

/// A second investigator, seated after the seer.
const ROLAND: InvestigatorId = InvestigatorId(2);

/// The seer's turn at `HERE`, with `other` seated after them.
fn board_with(seer: Seat, other: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(seer)
        .with_seat(other)
        .with_seating([INV, ROLAND])
        .open_turn(INV)
        .session()
}

/// On the investigator-deck branch the seer is asked which investigator's
/// deck, a selection anchored on each investigator card, and every
/// investigator is offered, the seer included.
#[test]
fn the_investigator_deck_branch_asks_which_investigator() {
    let session = board_with(
        seer(&[EMERGENCY_CACHE, GUTS, PERCEPTION]),
        seat(2).at(HERE).deck([OVERPOWER, GUTS, PERCEPTION]),
    );
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(INVESTIGATOR_DECK);

    assert_eq!(s.prompt().prompt, "Choose an investigator");
    assert_eq!(s.prompt().nature, PromptNature::Selection);
    assert_eq!(
        s.offered_targets(),
        [
            Some(s.state().investigator_card(INV)),
            Some(s.state().investigator_card(ROLAND))
        ],
    );
}

/// *"any investigator's deck"*: the seer reorders Roland's top 3, and the
/// seer's own deck is untouched.
#[test]
fn reordering_another_investigators_deck_leaves_yours_untouched() {
    let session = board_with(
        seer(&[EMERGENCY_CACHE, GUTS, PERCEPTION]),
        seat(2)
            .at(HERE)
            .deck([OVERPOWER, GUTS, PERCEPTION, EMERGENCY_CACHE]),
    );
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(INVESTIGATOR_DECK);
    let roland = s.state().investigator_card(ROLAND);
    let s = s.pick(roland);
    assert_eq!(
        s.offered_labels(),
        ["Overpower", "Guts", "Perception"],
        "Roland's top 3 are looked at",
    );
    assert_eq!(
        s.prompt().target,
        Some(OptionTarget::LookedAtTopOfDeck {
            investigator: ROLAND,
            count: 3,
        }),
    );
    assert_eq!(
        s.prompt().responder,
        Responder::Investigator(INV),
        "the seer looks, so the seer answers",
    );

    // Perception goes on top, then Overpower; Guts is left third.
    let s = s.pick(deck_card(ROLAND, 2));
    assert_eq!(s.offered_labels(), ["Overpower", "Guts"]);
    let s = s.pick(deck_card(ROLAND, 1));

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&ROLAND].deck,
        codes([PERCEPTION, OVERPOWER, GUTS, EMERGENCY_CACHE]),
        "Roland's top 3 are in the picked order",
    );
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes([EMERGENCY_CACHE, GUTS, PERCEPTION]),
        "the seer's own deck is untouched",
    );
}

/// With another investigator seated, the seer can still pick their own deck.
#[test]
fn picking_yourself_reorders_your_own_deck() {
    let session = board_with(
        seer(&[EMERGENCY_CACHE, GUTS, PERCEPTION]),
        seat(2).at(HERE).deck([OVERPOWER, GUTS, PERCEPTION]),
    );
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(INVESTIGATOR_DECK);
    let seer = s.state().investigator_card(INV);
    let s = s.pick(seer);
    assert_eq!(
        s.offered_labels(),
        ["Emergency Cache", "Guts", "Perception"]
    );

    // Guts on top, then Perception; Emergency Cache is left third.
    let s = s.pick(deck_card(INV, 1));
    let s = s.pick(deck_card(INV, 2));

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes([GUTS, PERCEPTION, EMERGENCY_CACHE]),
    );
    assert_eq!(
        s.state().investigators[&ROLAND].deck,
        codes([OVERPOWER, GUTS, PERCEPTION]),
        "Roland's deck is untouched",
    );
}

/// Elimination: *"The only manner in which eliminated investigators interact
/// with the game is when establishing "per investigator" values."* A defeated
/// Roland is not offered, so the seer is the one investigator left and the
/// choice binds without a prompt: the next prompt is the seer's own top 3.
#[test]
fn an_eliminated_investigator_is_not_offered() {
    let session = board_with(
        seer(&[EMERGENCY_CACHE, GUTS, PERCEPTION]),
        seat(2)
            .at(HERE)
            .deck([OVERPOWER, GUTS, PERCEPTION])
            .status(Status::Defeated),
    );
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(INVESTIGATOR_DECK);

    assert_eq!(
        s.offered_labels(),
        ["Emergency Cache", "Guts", "Perception"]
    );
}

#[test]
fn a_deck_of_one_card_takes_no_pick() {
    let session = board(seer(&[GUTS]), &[]);
    let scrying = scrying(&session);

    let s = session.take(&activate(scrying)).pick_nth(INVESTIGATOR_DECK);

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
