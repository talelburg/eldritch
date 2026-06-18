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

use std::sync::Once;

use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{CardCode, CardInPlay, CardInstanceId, InvestigatorId, LocationId, Phase};
use game_core::test_support::{test_investigator, test_location, GameStateBuilder};
use game_core::{apply, assert_event, Action, InputResponse, OptionId, PlayerAction};

const OLD_BOOK: &str = "01031";
const INV: InvestigatorId = InvestigatorId(1);
const LOC: LocationId = LocationId(10);
const BOOK_INST: CardInstanceId = CardInstanceId(0);

static INSTALL: Once = Once::new();
fn install() {
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Board: Old Book of Lore in play, the active investigator alone at `LOC` with
/// a known 4-card deck (top 3 distinct, plus a 4th below the searched region).
fn board() -> game_core::GameState {
    install();
    let mut inv = test_investigator(1);
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
        .with_location(test_location(10, "Study"))
        .with_active_investigator(INV)
        .with_turn_order([INV])
        .build()
}

fn activate(state: game_core::GameState) -> game_core::engine::ApplyResult {
    apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: INV,
            instance_id: BOOK_INST,
            ability_index: 0,
        }),
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
    assert_eq!(r.outcome, EngineOutcome::Done);
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
