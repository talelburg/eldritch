//! `Effect::PlaceEventsOnBottomOfDeck` against a mock registry: an
//! event leaving play goes to the bottom of its owner's deck, instead of their
//! discard pile, while the owner controls a card carrying the constant.
//!
//! The corpus carrier is Wendy's Amulet 01014, *"**Forced** - After you play an
//! event or discard an event from play: Place it on the bottom of your deck
//! instead of in your discard pile."*, covered end to end in
//! `crates/cards/tests/integration/wendys_amulet.rs`. No event Wendy can legally take stays
//! in play, so a synthetic in-play event and a synthetic carrier model the
//! primitive (ADR 0016). Its own test binary: the mock registry is built once
//! per process.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons};
use card_dsl::dsl::{self, Effect};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{CardCode, GameState, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, BoardLookup as _, MockRegistry, Seat};
use game_core::{assert_event, assert_no_event};

/// An event that sits in its owner's play area.
const STAYING_EVENT: &str = "TEST-STAYING-EVENT";
/// An asset carrying the constant.
const REDIRECTOR: &str = "TEST-REDIRECTOR";
/// An asset with no abilities.
const PLAIN_ASSET: &str = "TEST-PLAIN-ASSET";
/// Deck filler.
const FILLER: &str = "TEST-FILLER";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn event(code: &str) -> CardMetadata {
    test_support::test_card_metadata(
        code,
        code,
        CardKind::Event {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            play_only_during_turn: false,
        },
    )
}

fn asset(code: &str) -> CardMetadata {
    test_support::test_card_metadata(
        code,
        code,
        CardKind::Asset {
            class: Class::Neutral,
            cost: Some(0),
            xp: None,
            slots: vec![],
            health: None,
            sanity: None,
            skill_icons: SkillIcons::default(),
            is_fast: false,
            deck_limit: 2,
            uses: None,
            play_only_during_turn: false,
        },
    )
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(event(STAYING_EVENT))
            .with_card(asset(REDIRECTOR))
            .with_card(asset(PLAIN_ASSET))
            .with_abilities(STAYING_EVENT, Vec::new)
            .with_abilities(REDIRECTOR, || {
                vec![dsl::constant(Effect::PlaceEventsOnBottomOfDeck)]
            })
            .with_abilities(PLAIN_ASSET, Vec::new)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// `investigator`, at `HERE` with one card in their deck, on their open turn.
fn board(investigator: Seat) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(investigator.at(HERE).deck([FILLER]))
        .with_location(test_support::location(HERE))
        .open_turn(INV)
        .build()
}

/// Discard `code` from play through a native's exit, as a card-local native
/// would. Returns the state after and the exit's events.
fn discard_from_play(mut state: GameState, code: &str) -> (GameState, Vec<Event>) {
    let id = state.in_play(INV, code);
    let (outcome, events) = test_support::run_native(
        &mut state,
        registries().cards,
        |cx, _| {
            cx.discard_from_play(id).expect("the card leaves play");
            EngineOutcome::Done
        },
        &EvalContext::for_controller(INV),
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    (state, events)
}

fn codes(codes: &[&str]) -> Vec<CardCode> {
    codes.iter().copied().map(CardCode::new).collect()
}

#[test]
fn an_event_discarded_from_play_goes_to_the_bottom_of_its_owners_deck() {
    let state = board(seat(1).in_play([STAYING_EVENT, REDIRECTOR]));
    let (state, events) = discard_from_play(state, STAYING_EVENT);

    let inv = &state.investigators[&INV];
    assert_eq!(inv.deck, codes(&[FILLER, STAYING_EVENT]));
    assert!(inv.discard.is_empty());
    assert_eq!(
        events,
        vec![Event::CardPlacedOnBottomOfDeck {
            investigator: INV,
            code: CardCode::new(STAYING_EVENT),
        }],
        "one event per card, in place of the discard",
    );
}

#[test]
fn without_the_constant_an_event_discarded_from_play_is_discarded() {
    let state = board(seat(1).in_play([STAYING_EVENT]));
    let (state, events) = discard_from_play(state, STAYING_EVENT);

    let inv = &state.investigators[&INV];
    assert_eq!(inv.deck, codes(&[FILLER]));
    assert_eq!(inv.discard, codes(&[STAYING_EVENT]));
    assert_event!(events, Event::CardDiscarded { .. });
    assert_no_event!(events, Event::CardPlacedOnBottomOfDeck { .. });
}

/// The constant names events only: an asset discarded beside it goes to the
/// discard pile as ever.
#[test]
fn an_asset_discarded_from_play_is_discarded_beside_the_constant() {
    let state = board(seat(1).in_play([PLAIN_ASSET, REDIRECTOR]));
    let (state, events) = discard_from_play(state, PLAIN_ASSET);

    let inv = &state.investigators[&INV];
    assert_eq!(inv.deck, codes(&[FILLER]));
    assert_eq!(inv.discard, codes(&[PLAIN_ASSET]));
    assert_no_event!(events, Event::CardPlacedOnBottomOfDeck { .. });
}
