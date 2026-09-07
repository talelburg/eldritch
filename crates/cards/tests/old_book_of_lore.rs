//! #319 integration: Old Book of Lore 01031's `[action] Exhaust Old Book of
//! Lore: Choose an investigator at your location. That investigator searches
//! the top 3 cards of his or her deck for a card, draws it, and shuffles the
//! remaining cards into his or her deck.` end-to-end against the real
//! `cards::REGISTRY`.
//!
//! Solo: the "choose an investigator at your location" target auto-binds (one
//! co-located investigator), so the only suspend is the top-3 card pick. The
//! exhaust cost is paid before the effect runs.
//!
//! Own process → installs `cards::REGISTRY`.

use cards::REGISTRY;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::{self, ApplyResult, EngineOutcome, OptionId};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, GameState, InvestigatorId,
    LocationId, Phase,
};
use game_core::test_support::{self, GameStateBuilder};
use game_core::{assert_event, card_registry};

const OLD_BOOK: &str = "01031";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const BOOK_INST: CardInstanceId = CardInstanceId(0);

#[ctor::ctor(unsafe)]
fn install() {
    let _ = card_registry::install(REGISTRY);
}

/// Board: Old Book of Lore in play, the active investigator alone at `LOC` with
/// a known 4-card deck (top 3 distinct, plus a 4th below the searched region).
fn board() -> GameState {
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(OLD_BOOK), BOOK_INST));
    inv.deck = vec![
        CardCode::new("90001"),
        CardCode::new("90002"),
        CardCode::new("90003"),
        CardCode::new("90004"),
    ];

    GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator_at(inv, LOC)
        .with_location(test_support::test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .with_investigator_turn(INV)
        .build()
}

fn activate(state: GameState) -> ApplyResult {
    test_support::take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(BOOK_INST),
            address: AbilityAddress::Printed(0),
        },
    )
}

fn pick(state: GameState, option: u32) -> ApplyResult {
    engine::apply(
        state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(OptionId(option)),
        }),
    )
}

#[test]
fn action_searches_top_three_into_hand_then_shuffles() {
    // Activate: exhaust paid, target auto-binds (solo), top 3 give 3 eligible
    // ⇒ the card pick suspends.
    let r = activate(board());
    assert!(
        matches!(r.outcome, EngineOutcome::AwaitingInput { .. }),
        "top 3 ⇒ a card choice suspends",
    );
    assert!(
        r.state.investigators[&INV]
            .cards_in_play
            .iter()
            .any(|c| c.instance_id == BOOK_INST && c.exhausted),
        "exhaust cost paid before the effect",
    );

    // Pick option 1 → the second card of the top 3 ("90002").
    let r = pick(r.state, 1);
    assert!(matches!(r.outcome, EngineOutcome::AwaitingInput { .. }));
    let inv = &r.state.investigators[&INV];
    assert!(
        inv.hand.contains(&CardCode::new("90002")),
        "picked card moved to hand",
    );
    assert!(
        !inv.deck.contains(&CardCode::new("90002")),
        "picked card removed from deck",
    );
    assert_eq!(inv.deck.len(), 3, "one card left the deck");
    assert_event!(r.events, Event::CardSearchedToHand { .. });
    assert_event!(r.events, Event::DeckShuffled { .. });
}

/// #639 — RR "Ability": *"A triggered ability can only be initiated if its
/// effect has the potential to change the game state…"* With an empty deck
/// there is nothing to search, nothing to draw, and nothing to shuffle, so the
/// activation is rejected: the book stays ready and the action is not spent.
#[test]
fn an_empty_deck_cannot_be_searched() {
    let mut state = board();
    state
        .investigators
        .get_mut(&INV)
        .expect("seeded")
        .deck
        .clear();
    let actions_before = state.investigators[&INV].actions_remaining;

    // The turn menu already filters this out (`legal_actions` runs the same
    // validator); dispatch straight to the handler to prove *it* rejects a
    // directly-submitted activation.
    assert!(
        !enumerate::legal_actions(&state).contains(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(BOOK_INST),
            address: AbilityAddress::Printed(0),
        }),
        "the turn menu does not offer an activation the validator would reject",
    );
    let r = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(BOOK_INST),
            address: AbilityAddress::Printed(0),
        },
    );
    assert!(
        matches!(r.outcome, EngineOutcome::Rejected { .. }),
        "an empty deck ⇒ the search cannot change the game state: {:?}",
        r.outcome,
    );
    let inv = &r.state.investigators[&INV];
    assert!(
        inv.cards_in_play
            .iter()
            .any(|c| c.instance_id == BOOK_INST && !c.exhausted),
        "no exhaust paid on a reject",
    );
    assert_eq!(
        inv.actions_remaining, actions_before,
        "no action spent on a reject",
    );
}
