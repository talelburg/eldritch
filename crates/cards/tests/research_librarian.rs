//! #320 integration: Research Librarian 01032's `[reaction] After Research
//! Librarian enters play: Search your deck for a Tome asset and add it to your
//! hand. Shuffle your deck.` end-to-end against the real `cards::REGISTRY`.
//!
//! Exercises the `EnteredPlay` reaction window (the card enters play → its
//! self-referential reaction window opens) and the deck-search filter (entire
//! deck ∩ `Tome` asset). With two eligible Tomes the search suspends for a
//! pick, which also drives the choice-from-reaction reentrancy
//! (`resume_choice` re-driving the window).
//!
//! Own process → installs `cards::REGISTRY`.

use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, TestSession};

const LIBRARIAN: &str = "01032";
const OLD_BOOK: &str = "01031"; // Item. Tome. asset
const MEDICAL_TEXTS: &str = "01035"; // Item. Tome. asset
const GUTS: &str = "01089"; // a skill — not a Tome asset (filler)
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Board: the active investigator at `LOC` with Research Librarian in hand
/// (index 0) and `deck` as their deck.
fn board(deck: Vec<CardCode>) -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.hand = vec![CardCode::new(LIBRARIAN)];
    inv.deck = deck;

    GameStateBuilder::new()
        .with_investigator_at(inv, LOC)
        .with_location(test_support::test_location(10, "Study"))
        .open_turn(INV)
        .build()
}

fn play(state: GameState) -> TestSession {
    TestSession::new(state).take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    })
}

/// Research Librarian as its own reaction window offers it: the in-play
/// instance it entered play as.
fn librarian(session: &TestSession) -> OptionTarget {
    let instance = session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.code == CardCode::new(LIBRARIAN))
        .expect("Research Librarian is in play")
        .instance_id;
    OptionTarget::CardInstance(instance)
}

/// Take the `position`th eligible card from the search. A deck search's
/// options have no board home (ADR 0015 excludes them), so the pick is
/// positional.
fn take_searched(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

fn at_turn_menu(session: &TestSession) -> bool {
    session.prompt().target == Some(OptionTarget::TurnControl(INV))
}

#[test]
fn entering_play_tutors_the_only_tome_asset() {
    // Deck has exactly one Tome asset (Old Book) + non-Tome filler.
    let s = play(board(vec![
        CardCode::new(OLD_BOOK),
        CardCode::new(GUTS),
        CardCode::new(GUTS),
    ]));
    // Research Librarian entered play → its EnteredPlay reaction window opened.
    assert!(s.prompt().skippable, "EnteredPlay reaction window opens");

    // Fire the reaction. One eligible Tome ⇒ the search auto-takes (no second
    // prompt) ⇒ back to the turn menu.
    let target = librarian(&s);
    let s = s.pick(target);
    assert!(at_turn_menu(&s));
    let inv = &s.state().investigators[&INV];
    assert!(
        inv.hand.contains(&CardCode::new(OLD_BOOK)),
        "the Tome asset was added to hand",
    );
    assert!(
        !inv.deck.contains(&CardCode::new(OLD_BOOK)),
        "and removed from the deck",
    );
    assert_event!(s.events(), Event::CardSearchedToHand { .. });
    assert_event!(s.events(), Event::DeckShuffled { .. });
}

/// #639: the initiation gate reaches the reaction path too. `Effect::SearchDeck`
/// against an **empty** deck is provably inert — nothing to find, nothing to
/// shuffle — so RR "Ability" (*"a triggered ability can only be initiated if its
/// effect has the potential to change the game state…"*) bars the reaction and
/// no window offers it. Research Librarian still enters play normally.
#[test]
fn an_empty_deck_does_not_open_the_tutor_reaction() {
    let s = play(board(Vec::new()));
    assert!(
        s.state().investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(LIBRARIAN)),
        "Research Librarian still enters play — only its reaction is barred",
    );
    assert!(
        at_turn_menu(&s),
        "no reaction offered for a search that cannot change the game state: {:?}",
        s.prompt(),
    );
}

/// A **non-empty** deck with no eligible Tome is deliberately *not* proven inert
/// — the search's mandatory shuffle still reorders the deck — so the reaction is
/// offered and simply finds nothing (#639's conservative posture).
#[test]
fn a_tomeless_but_non_empty_deck_still_offers_the_reaction() {
    let s = play(board(vec![CardCode::new(GUTS), CardCode::new(GUTS)]));
    assert!(
        s.prompt().skippable,
        "EnteredPlay reaction window opens even with no eligible Tome",
    );
    // Fire it: 0 eligible cards ⇒ find nothing, shuffle anyway.
    let target = librarian(&s);
    let s = s.pick(target);
    assert!(
        !s.state().investigators[&INV]
            .hand
            .contains(&CardCode::new(GUTS)),
        "no Tome to find, so nothing was tutored",
    );
    assert_event!(s.events(), Event::DeckShuffled { .. });
}

#[test]
fn two_tome_assets_prompt_a_choice_then_tutor_the_pick() {
    // Two eligible Tome assets (Old Book at eligible index 0, Medical Texts at
    // index 1, deck order preserved) + non-Tome filler between them.
    let s = play(board(vec![
        CardCode::new(OLD_BOOK),
        CardCode::new(GUTS),
        CardCode::new(MEDICAL_TEXTS),
    ]));
    assert!(s.prompt().skippable, "EnteredPlay reaction window opens");

    // Fire the reaction → SearchDeck sees 2 eligible Tomes ⇒ suspends for a
    // card pick.
    let target = librarian(&s);
    let s = s.pick(target);
    assert!(
        !at_turn_menu(&s) && !s.prompt().skippable,
        "2 eligible Tomes ⇒ the search suspends for a pick",
    );

    // Pick the second eligible Tome (Medical Texts). On Done, resume_choice
    // re-drives the still-open reaction window so it closes (Task 4).
    let s = s.apply(take_searched(1));
    let inv = &s.state().investigators[&INV];
    assert!(
        inv.hand.contains(&CardCode::new(MEDICAL_TEXTS)),
        "the picked Tome was added to hand",
    );
    assert!(
        !inv.deck.contains(&CardCode::new(MEDICAL_TEXTS)),
        "and removed from the deck",
    );
    assert!(
        inv.deck.contains(&CardCode::new(OLD_BOOK)),
        "the unpicked Tome stays in the deck",
    );
    assert_event!(s.events(), Event::DeckShuffled { .. });
    assert!(at_turn_menu(&s), "the reaction window closed");
}
