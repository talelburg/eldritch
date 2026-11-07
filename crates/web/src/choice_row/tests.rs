use game_core::engine::{ChoiceOption, OptionId, OptionTarget};
use game_core::state::{DiscardIndex, GameStateBuilder};
use game_core::test_support::seat;

use super::*;

const ROLAND: InvestigatorId = InvestigatorId(1);

/// Roland with `cards` cards in his deck.
fn game(cards: usize) -> GameState {
    GameStateBuilder::new()
        .with_seat(seat(1).deck(std::iter::repeat_n("01089", cards)))
        .build()
}

/// A prompt offering nothing: a search's row reads nothing from its prompt.
fn no_request() -> InputRequest {
    InputRequest::pick_single("Search: choose a card to take", Vec::new())
}

/// Scavenging 01073: *"Choose an \[\[Item\]\] card in your discard pile and
/// add it to your hand."* A discard pile is open information, so everyone else
/// sees the Items on offer face up, top of the pile first, and not the pile's
/// other cards.
#[test]
fn a_discard_pile_shows_the_offered_cards_face_up() {
    // Bottom first: Medical Texts, Guts, Magnifying Glass, Perception.
    let game = GameStateBuilder::new()
        .with_seat(seat(1).discard(["01035", "01089", "01030", "01090"]))
        .build();
    let take = |id: u32, index: u8| {
        ChoiceOption::new(OptionId(id), "an Item").at(OptionTarget::DiscardCard {
            investigator: ROLAND,
            index: DiscardIndex(index),
        })
    };
    let request = InputRequest::pick_single(
        "Scavenging: choose an Item to add to your hand",
        vec![take(0, 0), take(1, 2)],
    )
    .at(OptionTarget::DiscardPile(ROLAND));
    assert_eq!(
        ChoiceRow::of(CardArea::DiscardPile(ROLAND), &request, &game),
        ChoiceRow {
            caption: "being chosen from".to_owned(),
            cards: RowCards::FaceUp(vec![CardCode::new("01030"), CardCode::new("01035")]),
        },
    );
}

/// Old Book of Lore 01031 searches *"the top 3 cards"*: everyone else sees 3
/// face-down cards being searched.
#[test]
fn a_searched_top_shows_one_face_down_card_per_card_in_it() {
    let area = CardArea::SearchedTopOfDeck {
        investigator: ROLAND,
        count: 3,
    };
    assert_eq!(
        ChoiceRow::of(area, &no_request(), &game(27)),
        ChoiceRow {
            caption: "being searched".to_owned(),
            cards: RowCards::FaceDown(3, Back::Player),
        },
    );
}

/// Research Librarian 01032 searches *"your deck"*: everyone else sees one
/// face-down deck, with how many cards it holds.
#[test]
fn a_searched_deck_shows_one_face_down_deck_with_its_count() {
    let area = CardArea::SearchedDeck(ROLAND);
    assert_eq!(
        ChoiceRow::of(area, &no_request(), &game(27)),
        ChoiceRow {
            caption: "being searched: 27 cards".to_owned(),
            cards: RowCards::Deck,
        },
    );
    assert_eq!(
        ChoiceRow::of(area, &no_request(), &game(1)).caption,
        "being searched: 1 card"
    );
}

/// Scrying 01061 *looks at* the top 3 of a deck: everyone else sees the 3
/// face down, being looked at. Not a search (FAQ 2.13), and hidden anyway, as
/// the table would play it.
#[test]
fn a_looked_at_top_shows_one_player_back_per_card_being_looked_at() {
    let area = CardArea::LookedAtTopOfDeck {
        investigator: ROLAND,
        count: 2,
    };
    assert_eq!(
        ChoiceRow::of(area, &no_request(), &game(2)),
        ChoiceRow {
            caption: "being looked at".to_owned(),
            cards: RowCards::FaceDown(2, Back::Player),
        },
    );
}

/// Scrying on the encounter deck: the cards show encounter backs.
#[test]
fn the_encounter_decks_looked_at_top_shows_encounter_backs() {
    let area = CardArea::LookedAtTopOfEncounterDeck { count: 3 };
    assert_eq!(
        ChoiceRow::of(area, &no_request(), &game(0)),
        ChoiceRow {
            caption: "being looked at".to_owned(),
            cards: RowCards::FaceDown(3, Back::Encounter),
        },
    );
}
