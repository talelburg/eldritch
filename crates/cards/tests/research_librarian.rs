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
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const LIBRARIAN: &str = "01032";
const OLD_BOOK: &str = "01031"; // Item. Tome. asset
const MEDICAL_TEXTS: &str = "01035"; // Item. Tome. asset
const GUTS: &str = "01089"; // a skill — not a Tome asset (filler)
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Board: the active investigator's turn at `HERE`, with Research Librarian
/// in hand (index 0) and `deck` as their deck.
fn board(deck: &[&str]) -> TestSession {
    GameStateBuilder::new()
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([LIBRARIAN])
                .deck(deck.iter().copied()),
        )
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
}

fn play(session: TestSession) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    })
}

/// Research Librarian as its own reaction window offers it: the in-play
/// instance it entered play as.
fn librarian(session: &TestSession) -> OptionTarget {
    session.state().card(INV, LIBRARIAN)
}

/// Take the `position`th eligible card from the search.
///
/// Exempt from the anchor-pick rule, and sent raw: a deck search's options
/// are cards **in a deck**, with no board anchor and neither of ADR 0015's
/// natures, which that ADR knowingly excludes and defers. No session step can
/// name one, so the pick is positional.
fn take_searched(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

#[test]
fn entering_play_tutors_the_only_tome_asset() {
    // Deck has exactly one Tome asset (Old Book) + non-Tome filler.
    let s = play(board(&[OLD_BOOK, GUTS, GUTS]));
    // Research Librarian entered play → its EnteredPlay reaction window opened.
    assert!(s.prompt().skippable, "EnteredPlay reaction window opens");

    // Fire the reaction. One eligible Tome ⇒ the search auto-takes (no second
    // prompt) ⇒ back to the turn menu.
    let target = librarian(&s);
    let s = s.pick(target);
    s.assert_at_turn_menu(INV);
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
    let s = play(board(&[]));
    assert!(
        s.state().investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.code == CardCode::new(LIBRARIAN)),
        "Research Librarian still enters play — only its reaction is barred",
    );
    // No reaction offered for a search that cannot change the game state.
    s.assert_at_turn_menu(INV);
}

/// A **non-empty** deck with no eligible Tome is deliberately *not* proven inert
/// — the search's mandatory shuffle still reorders the deck — so the reaction is
/// offered and simply finds nothing (#639's conservative posture).
#[test]
fn a_tomeless_but_non_empty_deck_still_offers_the_reaction() {
    let s = play(board(&[GUTS, GUTS]));
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
    let s = play(board(&[OLD_BOOK, GUTS, MEDICAL_TEXTS]));
    assert!(s.prompt().skippable, "EnteredPlay reaction window opens");

    // Fire the reaction → SearchDeck sees 2 eligible Tomes ⇒ suspends for a
    // card pick.
    let target = librarian(&s);
    let s = s.pick(target);
    assert!(
        s.prompt().target != Some(OptionTarget::TurnControl(INV)) && !s.prompt().skippable,
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
    // The reaction window closed.
    s.assert_at_turn_menu(INV);
}
