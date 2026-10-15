//! C6c (#243) integration: the neutral cards' new effect shapes end-to-end
//! against the real `cards::REGISTRY`.
//!
//! - Emergency Cache 01088: `on_play(gain_resources(You, 3))`.
//! - Guts 01089: `on_skill_test_resolution(Success, draw_cards(You, 1))` —
//!   draws on a successful committed-to test, not on a failed one.
//!
//! The other three draw-skills (Perception/Overpower/Manual Dexterity) are
//! structurally identical to Guts; their impl unit tests cover them.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, Seat, TestSession};
use game_core::{assert_event, assert_no_event};

const EMERGENCY_CACHE: &str = "01088";
const GUTS: &str = "01089";
const INV: InvestigatorId = InvestigatorId(1);

#[test]
fn emergency_cache_play_gains_three_resources() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).hand([EMERGENCY_CACHE]))
        .open_turn(INV)
        .session();
    let before = session.state().investigators[&INV].resources;

    let session = session.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });

    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::ResourcesGained { amount: 3, .. });
    assert_eq!(session.state().investigators[&INV].resources, before + 3);
}

/// Investigator 1 holding `GUTS` + spare deck cards.
fn holding_guts() -> Seat {
    seat(1).hand([GUTS]).deck(["spare-1", "spare-2"])
}

/// `investigator`'s turn with a chaos bag of `token` (`Numeric(0)` → success
/// vs difficulty 1; `AutoFail` → failure).
fn guts_board(investigator: Seat, token: ChaosToken) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

/// A willpower (1) test, committing Guts to it.
fn perform_and_commit_guts(session: TestSession) -> TestSession {
    session
        .test_skill(INV, SkillKind::Willpower, 1)
        .commit(&[GUTS])
}

#[test]
fn guts_draws_a_card_on_a_successful_test() {
    // willpower 3 + Numeric(0) vs difficulty 1 → success → draw 1.
    let s = perform_and_commit_guts(guts_board(holding_guts(), ChaosToken::Numeric(0)));
    s.assert_at_turn_menu(INV);
    assert_event!(s.finish().events, Event::CardsDrawn { count: 1, .. });
}

#[test]
fn guts_draws_nothing_on_a_failed_test() {
    // AutoFail → failure → no draw.
    let s = perform_and_commit_guts(guts_board(holding_guts(), ChaosToken::AutoFail));
    s.assert_at_turn_menu(INV);
    assert_no_event!(s.finish().events, Event::CardsDrawn { .. });
}

/// A card-effect draw obeys the empty-deck rule like the Draw action does
/// (#636). Rules Reference, glossary "Drawing Cards": "If an investigator
/// with an empty investigator deck needs to draw a card, that investigator
/// shuffles his or her discard pile back into his or her deck, then draws
/// the card, and upon completion of the entire draw takes one horror."
#[test]
fn guts_on_an_empty_deck_reshuffles_the_discard_and_takes_one_horror() {
    let empty_deck = seat(1).hand([GUTS]).discard(["spare-1", "spare-2"]);

    let s = perform_and_commit_guts(guts_board(empty_deck, ChaosToken::Numeric(0)));

    s.assert_at_turn_menu(INV);
    let r = s.finish();
    assert_event!(r.events, Event::DeckShuffled { .. });
    assert_event!(r.events, Event::CardsDrawn { count: 1, .. });
    assert_event!(r.events, Event::HorrorTaken { amount: 1, .. });
    let inv = &r.state.investigators[&INV];
    assert_eq!(
        inv.discard,
        vec![CardCode::new(GUTS)],
        "the two discarded cards shuffled back into the deck; only the \
         just-resolved Guts remains in the discard",
    );
    assert_eq!(inv.deck.len(), 1, "two cards returned, one of them drawn");
    assert_eq!(inv.horror(), 1, "deck-out costs 1 horror, once");
}
