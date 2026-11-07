use game_core::engine::{ChoiceOption, Responder};
use game_core::state::{DeckIndex, GameStateBuilder};
use game_core::test_support::{self, seat};

use super::*;
use crate::decision;

const ROLAND: InvestigatorId = InvestigatorId(1);
/// Roland's deck, top first.
const DECK: [&str; 5] = [
    GUTS,
    OLD_BOOK_OF_LORE,
    "01088", // Emergency Cache
    MEDICAL_TEXTS,
    "01090", // Perception
];
const GUTS: &str = "01089";
const OLD_BOOK_OF_LORE: &str = "01031";
const MEDICAL_TEXTS: &str = "01035";

fn game() -> GameState {
    GameStateBuilder::new()
        .with_seat(seat(1).deck(DECK))
        .build()
}

/// The option `id` taking the card `index` cards below the top of Roland's
/// deck.
fn deck_card(id: u32, index: u8) -> ChoiceOption {
    ChoiceOption::new(OptionId(id), "a card").at(OptionTarget::DeckCard {
        investigator: ROLAND,
        index: DeckIndex(index),
    })
}

fn top(count: u8) -> OptionTarget {
    OptionTarget::SearchedTopOfDeck {
        investigator: ROLAND,
        count,
    }
}

/// A search prompt over `area` offering `options`, answered by Roland.
fn search(area: OptionTarget, options: Vec<ChoiceOption>) -> InputRequest {
    InputRequest::pick_single("Search: choose a card to take", options)
        .at(area)
        .answered_by(Responder::Investigator(ROLAND))
}

/// The client of the account holding Roland, with `request` live.
fn holding_roland(request: InputRequest) -> ClientState {
    ClientState {
        game: Some(game()),
        outcome: Some(test_support::awaiting_request(request)),
        ..Default::default()
    }
    .you_hold_every_investigator()
}

fn code(code: &str) -> CardCode {
    CardCode::new(code)
}

fn offered(code: &str, id: u32) -> ModalCard {
    ModalCard {
        code: CardCode::new(code),
        option: Some(OptionId(id)),
    }
}

fn passed_over(code: &str) -> ModalCard {
    ModalCard {
        code: CardCode::new(code),
        option: None,
    }
}

/// Old Book of Lore 01031: *"searches the top 3 cards of his or her deck for a
/// card, draws it"*. Every one of the 3 is offered, with Draw.
#[test]
fn a_search_of_the_top_three_shows_each_card_with_draw() {
    let state = holding_roland(search(
        top(3),
        vec![deck_card(0, 0), deck_card(1, 1), deck_card(2, 2)],
    ));
    let modal = live_card_modal(&state).expect("the card modal is up");
    assert_eq!(modal.verb(), Verb::Draw);
    assert_eq!(modal.verb().label(), "Draw");
    assert_eq!(
        modal.shown,
        [
            offered(GUTS, 0),
            offered(OLD_BOOK_OF_LORE, 1),
            offered("01088", 2)
        ],
        "the top 3, top first, each read from the deck at its anchor",
    );
    assert!(modal.collapsed.is_empty());
    assert_eq!(modal.collapsed_label(), None);
    assert!(modal_is_live(&state));
}

/// Arcane Initiate 01063 with one Spell among the top 3: all 3 are shown, so
/// the player sees what they pass over, and only the Spell carries Draw.
#[test]
fn a_top_three_search_offering_one_card_still_shows_all_three() {
    let state = holding_roland(search(top(3), vec![deck_card(0, 1)]));
    let modal = live_card_modal(&state).expect("the card modal is up");
    assert_eq!(
        modal.shown,
        [
            passed_over(GUTS),
            offered(OLD_BOOK_OF_LORE, 0),
            passed_over("01088")
        ],
    );
    assert!(
        modal.collapsed.is_empty(),
        "a top-N area folds nothing away"
    );
}

/// Research Librarian 01032 searches the whole deck for a Tome: the Tomes are
/// shown with Draw, and the rest of the deck folds under a count.
#[test]
fn a_whole_deck_search_shows_the_offered_cards_and_collapses_the_rest() {
    let state = holding_roland(search(
        OptionTarget::PlayerDeck(ROLAND),
        vec![deck_card(0, 1), deck_card(1, 3)],
    ));
    let modal = live_card_modal(&state).expect("the card modal is up");
    assert_eq!(
        modal.shown,
        [offered(OLD_BOOK_OF_LORE, 0), offered(MEDICAL_TEXTS, 1)],
    );
    assert_eq!(modal.collapsed, [code(GUTS), code("01088"), code("01090")]);
    assert_eq!(
        modal.collapsed_label().as_deref(),
        Some("3 other cards searched")
    );
}

#[test]
fn one_card_left_over_is_counted_in_the_singular() {
    let game = GameStateBuilder::new()
        .with_seat(seat(1).deck([OLD_BOOK_OF_LORE, MEDICAL_TEXTS, GUTS]))
        .build();
    let state = ClientState {
        game: Some(game),
        outcome: Some(test_support::awaiting_request(search(
            OptionTarget::PlayerDeck(ROLAND),
            vec![deck_card(0, 0), deck_card(1, 1)],
        ))),
        ..Default::default()
    }
    .you_hold_every_investigator();
    let modal = live_card_modal(&state).expect("the card modal is up");
    assert_eq!(
        modal.collapsed_label().as_deref(),
        Some("1 other card searched")
    );
}

fn looked_at(count: u8) -> OptionTarget {
    OptionTarget::LookedAtTopOfDeck {
        investigator: ROLAND,
        count,
    }
}

/// Scrying 01061: *"Look at the top 3 cards of any investigator's deck or the
/// encounter deck. Return them to the top of that deck in any order."* Looking
/// at is not a search (FAQ 2.13), so the verb is Place next, not Draw. On the
/// first pick every card is unplaced.
#[test]
fn scrying_a_deck_shows_the_three_cards_with_place_next() {
    let state = holding_roland(search(
        looked_at(3),
        vec![deck_card(0, 0), deck_card(1, 1), deck_card(2, 2)],
    ));
    let modal = live_card_modal(&state).expect("the card modal is up");
    assert_eq!(modal.verb(), Verb::PlaceNext);
    assert_eq!(modal.verb().label(), "Place next");
    assert_eq!(
        modal.shown,
        [
            offered(GUTS, 0),
            offered(OLD_BOOK_OF_LORE, 1),
            offered("01088", 2)
        ],
    );
    assert!(modal.placed.is_empty(), "nothing is placed yet");
    assert!(modal.collapsed.is_empty());
}

/// Scrying's second pick: the engine has moved the first card placed to the
/// top, and the area is still the 3 looked at. The top position, not offered,
/// is the card placed so far; the 2 below it are still to place.
#[test]
fn scryings_second_pick_shows_the_placed_card_apart_from_the_two_left() {
    let state = holding_roland(search(looked_at(3), vec![deck_card(0, 1), deck_card(1, 2)]));
    let modal = live_card_modal(&state).expect("the card modal is up");
    assert_eq!(modal.placed, [code(GUTS)]);
    assert_eq!(
        modal.shown,
        [offered(OLD_BOOK_OF_LORE, 0), offered("01088", 1)],
        "only the unplaced cards are shown, each with Place next",
    );
    assert!(modal.collapsed.is_empty());
}

/// Scrying on the encounter deck reads the encounter deck's top cards, which
/// the modal shows by name and text.
#[test]
fn scrying_the_encounter_deck_reads_its_top_cards() {
    let ancient_evils = "01166";
    let crypt_chill = "01167";
    let obscuring_fog = "01168";
    let game = GameStateBuilder::new()
        .with_seat(seat(1).deck(DECK))
        .with_encounter_deck([ancient_evils, crypt_chill, obscuring_fog, "01163"])
        .build();
    let encounter_card = |id: u32, index: u8| {
        ChoiceOption::new(OptionId(id), "a card").at(OptionTarget::EncounterDeckCard {
            index: DeckIndex(index),
        })
    };
    let state = ClientState {
        game: Some(game),
        outcome: Some(test_support::awaiting_request(search(
            OptionTarget::LookedAtTopOfEncounterDeck { count: 3 },
            vec![encounter_card(0, 1), encounter_card(1, 2)],
        ))),
        ..Default::default()
    }
    .you_hold_every_investigator();
    let modal = live_card_modal(&state).expect("the card modal is up");
    assert_eq!(modal.verb(), Verb::PlaceNext);
    assert_eq!(modal.placed, [code(ancient_evils)]);
    assert_eq!(
        modal.shown,
        [offered(crypt_chill, 0), offered(obscuring_fog, 1)],
    );
}

/// An account that doesn't answer the search gets no modal: the searched cards
/// are the searcher's to look at (`glossary/Search.md`).
#[test]
fn an_account_that_does_not_answer_gets_no_modal() {
    let request = search(top(3), vec![deck_card(0, 0), deck_card(1, 1)]);
    let viewer = ClientState {
        game: Some(game()),
        outcome: Some(test_support::awaiting_request(request.clone())),
        ..Default::default()
    };
    assert_eq!(live_card_modal(&viewer), None);
    assert!(!modal_is_live(&viewer));
    assert_eq!(
        CardArea::of(&request),
        Some(CardArea::SearchedTopOfDeck {
            investigator: ROLAND,
            count: 3,
        }),
        "the area is still read from the prompt, for the face-down view",
    );
}

/// A prompt whose request names a deck but whose options aren't cards in it,
/// or a decision, keeps its own surface.
#[test]
fn other_prompts_get_no_card_modal() {
    let unanchored = holding_roland(search(top(3), vec![ChoiceOption::new(OptionId(0), "Guts")]));
    assert_eq!(live_card_modal(&unanchored), None);

    let someone_elses = holding_roland(search(
        top(3),
        vec![
            ChoiceOption::new(OptionId(0), "Guts").at(OptionTarget::DeckCard {
                investigator: InvestigatorId(2),
                index: DeckIndex(0),
            }),
        ],
    ));
    assert_eq!(live_card_modal(&someone_elses), None);

    let decision = holding_roland(
        InputRequest::pick_single("Choose one", vec![ChoiceOption::new(OptionId(0), "Heal")])
            .deciding()
            .answered_by(Responder::Investigator(ROLAND)),
    );
    assert_eq!(live_card_modal(&decision), None);
    assert!(decision::modal_is_live(&decision));
}
