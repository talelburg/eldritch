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
//! Each per-card pick is sent raw: its options are cards **in a deck**, with no
//! board anchor and neither of ADR 0015's natures, so no session step can name
//! one and the pick is positional (Old Book of Lore's test does the same).
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::card_data::UseKind;
use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget, PromptNature};
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInstanceId, GameStateBuilder, InvestigatorId,
    LocationId, Status,
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

/// The deck decision's branches, in printed order. The investigator-deck
/// branch is the seer's own deck whenever the seer is alone.
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

/// Put the `position`th of the cards still to be placed next from the top.
fn place(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        investigator: INV,
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

fn card_anchor(session: &TestSession, investigator: InvestigatorId) -> OptionTarget {
    session.state().investigators[&investigator].card_anchor()
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

    let s = session.take(&activate(scrying)).pick_nth(YOUR_DECK);

    assert_eq!(s.prompt().prompt, "Choose an investigator");
    assert_eq!(s.prompt().nature, PromptNature::Selection);
    assert_eq!(
        s.offered_targets(),
        [Some(card_anchor(&s, INV)), Some(card_anchor(&s, ROLAND))],
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

    let s = session.take(&activate(scrying)).pick_nth(YOUR_DECK);
    let roland = card_anchor(&s, ROLAND);
    let s = s.pick(roland);
    assert_eq!(
        labels(&s),
        ["Overpower", "Guts", "Perception"],
        "Roland's top 3 are looked at",
    );

    // Perception goes on top, then Overpower; Guts is left third.
    let s = s.apply(place(2));
    assert_eq!(labels(&s), ["Overpower", "Guts"]);
    let s = s.apply(place(0));

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

    let s = session.take(&activate(scrying)).pick_nth(YOUR_DECK);
    let seer = card_anchor(&s, INV);
    let s = s.pick(seer);
    assert_eq!(labels(&s), ["Emergency Cache", "Guts", "Perception"]);

    // Guts on top, then Perception; Emergency Cache is left third.
    let s = s.apply(place(1));
    let s = s.apply(place(1));

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

    let s = session.take(&activate(scrying)).pick_nth(YOUR_DECK);

    assert_eq!(labels(&s), ["Emergency Cache", "Guts", "Perception"]);
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
