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

use game_core::engine::EngineOutcome;
use game_core::engine::TurnAction;
use game_core::event::Event;
use game_core::state::{CardCode, Continuation, InvestigatorId, LocationId, Phase};
use game_core::test_support::{
    take_turn_action, test_investigator, test_location, GameStateBuilder,
};
use game_core::{apply, assert_event, Action, InputResponse, OptionId, PlayerAction};

const LIBRARIAN: &str = "01032";
const OLD_BOOK: &str = "01031"; // Item. Tome. asset
const MEDICAL_TEXTS: &str = "01035"; // Item. Tome. asset
const GUTS: &str = "01089"; // a skill — not a Tome asset (filler)
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Board: the active investigator at `LOC` with Research Librarian in hand
/// (index 0) and `deck` as their deck.
fn board(deck: Vec<CardCode>) -> game_core::GameState {
    let mut inv = test_investigator(1);
    inv.hand = vec![CardCode::new(LIBRARIAN)];
    inv.deck = deck;

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

fn play(state: game_core::GameState) -> game_core::engine::ApplyResult {
    take_turn_action(
        state,
        &TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        },
    )
}

fn pick(state: game_core::GameState, option: u32) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(option)),
        }),
    )
}

#[test]
fn entering_play_tutors_the_only_tome_asset() {
    // Deck has exactly one Tome asset (Old Book) + non-Tome filler.
    let r = play(board(vec![
        CardCode::new(OLD_BOOK),
        CardCode::new(GUTS),
        CardCode::new(GUTS),
    ]));
    // Research Librarian entered play → its EnteredPlay reaction window opened.
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "EnteredPlay reaction window opens",
    );

    // Fire the reaction (option 0 = the sole pending trigger). One eligible
    // Tome ⇒ the search auto-takes (no second prompt) ⇒ Done.
    let r = pick(r.state, 0);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    let inv = &r.state.investigators[&INV];
    assert!(
        inv.hand.contains(&CardCode::new(OLD_BOOK)),
        "the Tome asset was added to hand",
    );
    assert!(
        !inv.deck.contains(&CardCode::new(OLD_BOOK)),
        "and removed from the deck",
    );
    assert_event!(r.events, Event::CardSearchedToHand { .. });
    assert_event!(r.events, Event::DeckShuffled { .. });
}

#[test]
fn two_tome_assets_prompt_a_choice_then_tutor_the_pick() {
    // Two eligible Tome assets (Old Book at eligible index 0, Medical Texts at
    // index 1, deck order preserved) + non-Tome filler between them.
    let r = play(board(vec![
        CardCode::new(OLD_BOOK),
        CardCode::new(GUTS),
        CardCode::new(MEDICAL_TEXTS),
    ]));
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "EnteredPlay reaction window opens",
    );

    // Fire the reaction → SearchDeck sees 2 eligible Tomes ⇒ suspends for a
    // card pick.
    let r = pick(r.state, 0);
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "2 eligible Tomes ⇒ the search suspends for a pick",
    );

    // Pick the second eligible Tome (Medical Texts). On Done, resume_choice
    // re-drives the still-open reaction window so it closes (Task 4).
    let r = pick(r.state, 1);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    let inv = &r.state.investigators[&INV];
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
    assert_event!(r.events, Event::DeckShuffled { .. });
    assert!(
        r.state
            .continuations
            .last()
            .and_then(Continuation::pending_candidates)
            .is_none_or(Vec::is_empty),
        "the reaction window closed",
    );
}
