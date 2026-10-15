//! Wendy's Amulet 01014 end to end against the real card registry (#1109,
//! #1112). Card text (verbatim, `data/arkhamdb-snapshot/pack/core/core.json`):
//!
//! > Wendy Adams deck only.
//! > You may play the topmost event in your discard pile as if it were in your hand.
//! > <b>Forced</b> - After you play an event or discard an event from play: Place it on the bottom of your deck instead of in your discard pile.
//!
//! Rulings (<https://arkhamdb.com/card/01014>):
//!
//! > If your deck is empty, and Wendy’s Amulet would have you place an event
//! > “on the bottom of your deck,” that event becomes your new deck.
//!
//! > 'Play' and 'Commit' are two different game terms. Wendy's Amulet allows
//! > you to 'play' the topmost event card in your discard pile, but it does not
//! > allow you to 'commit' it to a skill test.
//!
//! The redirect is tested at both of its sites: a played event's step-4
//! placement, and an event leaving play (Barricade 01038, which stays in play
//! attached to a location). The play permission is tested from the turn menu
//! and from a reaction window ("Look what I found!" 01079), each option
//! anchored on the Amulet.
//!
//! Runs against `cards::TEST_REGISTRIES`.

use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{
    Agenda, CardCode, ChaosBag, ChaosToken, GameStateBuilder, InvestigatorId, Location, LocationId,
    SkillKind, TokenModifiers,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

const AMULET: &str = "01014";
/// Event. `Supply.` Gain 3 resources.
const EMERGENCY_CACHE: &str = "01088";
/// Event. `Spell.` Places 1 doom on the current agenda.
const DARK_MEMORY: &str = "01013";
const HEIRLOOM: &str = "01012";
/// Event that stays in play, attached to the location.
const BARRICADE: &str = "01038";
/// Deck filler, and a non-event discard card.
const GUTS: &str = "01089";
/// Fast event. *"Play after you fail a skill test by 2 or less while
/// investigating."* Two intellect icons, so it could be committed if a
/// discard-pile card could be.
const LOOK_WHAT_I_FOUND: &str = "01079";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(10);
const THERE: LocationId = LocationId(11);

const PLAY_FIRST: TurnAction = TurnAction::PlayCard {
    investigator: INV,
    source: PlaySource::Hand(0),
};

/// `investigator`'s open turn at `HERE`, solo, with The Gathering's first
/// agenda current (Dark Memory places its doom there).
fn board(investigator: Seat) -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
        .build();
    // The builder stages no agenda.
    state.agenda_deck = vec![Agenda {
        code: CardCode::new("01105"),
        doom_threshold: 3,
    }];
    state.agenda_index = 0;
    TestSession::new(state, TEST_REGISTRIES)
}

fn codes(codes: &[&str]) -> Vec<CardCode> {
    codes.iter().copied().map(CardCode::new).collect()
}

#[test]
fn a_played_event_goes_to_the_bottom_of_the_deck() {
    let s = board(
        seat(1)
            .at(HERE)
            .in_play([AMULET])
            .hand([EMERGENCY_CACHE])
            .deck([GUTS]),
    )
    .take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(
        inv.deck,
        codes(&[GUTS, EMERGENCY_CACHE]),
        "placed on the bottom"
    );
    assert!(inv.discard.is_empty(), "instead of in the discard pile");
    assert_event!(
        s.events(),
        Event::CardPlacedOnBottomOfDeck { investigator: INV, code } if code == &CardCode::new(EMERGENCY_CACHE)
    );
    assert_no_event!(s.events(), Event::CardDiscarded { .. });
    assert_event!(s.events(), Event::CardPlayed { .. });
}

#[test]
fn into_an_empty_deck_the_event_becomes_the_deck() {
    let s = board(seat(1).at(HERE).in_play([AMULET]).hand([EMERGENCY_CACHE])).take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.deck, codes(&[EMERGENCY_CACHE]));
    assert!(inv.discard.is_empty());
}

#[test]
fn without_the_amulet_a_played_event_is_discarded() {
    let s = board(seat(1).at(HERE).hand([EMERGENCY_CACHE]).deck([GUTS])).take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.deck, codes(&[GUTS]));
    assert_eq!(inv.discard, codes(&[EMERGENCY_CACHE]));
    assert_no_event!(s.events(), Event::CardPlacedOnBottomOfDeck { .. });
}

/// *"After **you** play"*, and *"**your** deck"*: another investigator's
/// Amulet does not reach investigator 1's event.
#[test]
fn another_investigators_amulet_does_not_redirect_your_event() {
    let s = TestSession::new(
        GameStateBuilder::new()
            .with_registries(TEST_REGISTRIES)
            .with_location(test_support::test_location(HERE.0, "Study"))
            .with_seat(seat(1).at(HERE).hand([EMERGENCY_CACHE]))
            .with_seat(seat(2).at(HERE).in_play([AMULET]))
            .with_turn_order([INV, INV2])
            .open_turn(INV)
            .build(),
        TEST_REGISTRIES,
    )
    .take(&PLAY_FIRST);

    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].discard,
        codes(&[EMERGENCY_CACHE])
    );
    assert!(s.state().investigators[&INV2].deck.is_empty());
}

/// Official FAQ: *"An effect that triggers “after you play” an event but does
/// not modify where the event goes (such as Heirloom of Hyberborea’s ability)
/// would naturally trigger just after step 4."* — so Heirloom's window opens
/// with Dark Memory already on the bottom of the deck, and its draw takes the
/// top card.
#[test]
fn an_after_you_play_reaction_still_fires_after_placement() {
    let s = board(
        seat(1)
            .at(HERE)
            .in_play([AMULET, HEIRLOOM])
            .hand([DARK_MEMORY])
            .resources(5)
            .deck([GUTS]),
    )
    .take(&PLAY_FIRST);

    let heirloom = s.state().card(INV, HEIRLOOM);
    assert_eq!(s.offered_at(&heirloom), 1, "Heirloom is offered");
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes(&[GUTS, DARK_MEMORY]),
        "Dark Memory is already on the bottom when the window opens",
    );

    let s = s.pick(heirloom);
    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.hand, codes(&[GUTS]), "Heirloom drew the top card");
    assert_eq!(inv.deck, codes(&[DARK_MEMORY]));
}

/// The from-play arm with a printed card: Barricade stays in play attached to
/// the location, and its own Forced discards it when an investigator leaves.
/// Barricade is a Seeker card Wendy cannot take; a test board is not
/// deckbuilding-validated.
#[test]
fn an_event_discarded_from_play_goes_to_the_bottom_of_the_deck() {
    let mut here = test_support::test_location(HERE.0, "Study");
    here.connections = vec![THERE];
    let mut there: Location = test_support::test_location(THERE.0, "Hallway");
    there.connections = vec![HERE];
    let s = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(here)
        .with_location(there)
        .with_seat(
            seat(1)
                .at(HERE)
                .in_play([AMULET])
                .hand([BARRICADE])
                .deck([GUTS]),
        )
        .open_turn(INV)
        .session()
        .take(&PLAY_FIRST);
    s.assert_at_turn_menu(INV);
    assert_eq!(
        s.state().investigators[&INV].deck,
        codes(&[GUTS]),
        "an attached event is not placed by its play",
    );

    let s = s.take(&TurnAction::Move {
        investigator: INV,
        destination: THERE,
    });

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.deck, codes(&[GUTS, BARRICADE]));
    assert!(inv.discard.is_empty());
    assert!(s.state().locations[&HERE].attachments.is_empty());
    assert_event!(
        s.events(),
        Event::CardPlacedOnBottomOfDeck { investigator: INV, code } if code == &CardCode::new(BARRICADE)
    );
    assert_no_event!(s.events(), Event::CardDiscarded { .. });
}

// --- Playing the topmost event in the discard pile (#1112) -----------------

/// The labels of the options anchored on the Amulet.
fn labels_at_amulet(s: &TestSession) -> Vec<String> {
    let amulet = s.state().card(INV, AMULET);
    s.prompt()
        .options
        .iter()
        .filter(|o| o.target.as_ref() == Some(&amulet))
        .map(|o| o.label.clone())
        .collect()
}

#[test]
fn in_her_turn_she_plays_the_top_event_of_her_discard_pile() {
    let s = board(
        seat(1)
            .at(HERE)
            .in_play([AMULET])
            .discard([GUTS, EMERGENCY_CACHE])
            .deck([GUTS]),
    );
    assert_eq!(
        labels_at_amulet(&s),
        ["Play Emergency Cache (Wendy's Amulet)"],
        "offered on the Amulet, naming the event",
    );
    let (resources, actions) = {
        let inv = &s.state().investigators[&INV];
        (inv.resources, inv.actions_remaining)
    };

    let permission = s.state().in_play(INV, AMULET);
    let s = s.take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::TopOfDiscardPile { permission },
    });

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.resources, resources + 3, "Emergency Cache resolved");
    assert_eq!(inv.discard, codes(&[GUTS]), "it left the discard pile");
    assert_eq!(
        inv.deck,
        codes(&[GUTS, EMERGENCY_CACHE]),
        "and the Forced placed it on the bottom of the deck",
    );
    assert_eq!(
        inv.actions_remaining,
        actions - 1,
        "a play from the menu is an action"
    );
    assert_event!(
        s.events(),
        Event::CardPlayed { investigator: INV, code } if code == &CardCode::new(EMERGENCY_CACHE)
    );
}

#[test]
fn a_non_event_top_card_is_not_offered() {
    // Emergency Cache is an event, but it is below the top.
    let s = board(
        seat(1)
            .at(HERE)
            .in_play([AMULET])
            .discard([EMERGENCY_CACHE, GUTS]),
    );
    assert!(labels_at_amulet(&s).is_empty(), "{:?}", s.prompt());
}

#[test]
fn without_the_amulet_the_discard_pile_offers_nothing() {
    let s = board(seat(1).at(HERE).discard([EMERGENCY_CACHE]));
    assert!(
        !s.prompt()
            .options
            .iter()
            .any(|o| o.label.contains("Emergency Cache")),
        "{:?}",
        s.prompt(),
    );
}

/// A shroud-4 Study with 3 clues, intellect 2, 5 resources and an empty hand:
/// an Investigate fails by 2.
fn failed_investigation_board(investigator: Seat) -> TestSession {
    let mut study = test_support::test_location(HERE.0, "Study");
    study.shroud = 4;
    study.clues = 3;
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(investigator)
        .with_location(study)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .open_turn(INV)
        .session()
}

#[test]
fn look_what_i_found_on_top_of_the_discard_pile_is_offered_in_its_window() {
    let s = failed_investigation_board(
        seat(1)
            .at(HERE)
            .skill(SkillKind::Intellect, 2)
            .resources(5)
            .in_play([AMULET])
            .discard([LOOK_WHAT_I_FOUND]),
    )
    .take(&TurnAction::Investigate { investigator: INV })
    .commit(&[]);
    assert_eq!(
        labels_at_amulet(&s),
        ["Play \"Look what I found!\" (Wendy's Amulet)"],
    );

    let amulet = s.state().card(INV, AMULET);
    let s = s.pick(amulet);

    s.assert_at_turn_menu(INV);
    let inv = &s.state().investigators[&INV];
    assert_eq!(inv.clues, 2, "discovered 2 clues");
    assert_eq!(inv.resources, 3, "its cost of 2 was paid");
    assert!(inv.discard.is_empty());
    assert_eq!(
        inv.deck,
        codes(&[LOOK_WHAT_I_FOUND]),
        "then went to the deck"
    );
}

#[test]
fn a_discard_pile_card_cannot_be_committed() {
    // The commit window's choices are hand positions: with an empty hand there
    // is no position for "Look what I found!" to be chosen at.
    let s = board(
        seat(1)
            .at(HERE)
            .in_play([AMULET])
            .discard([LOOK_WHAT_I_FOUND]),
    )
    .test_skill(INV, SkillKind::Intellect, 2);

    s.assert_rejects(|s| {
        s.apply(Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickMultiple {
                selected: vec![OptionId(0)],
            },
        }))
    });
}
