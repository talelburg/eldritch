//! The Necronomicon (John Dee Translation) 01009, Daisy Walker's signature
//! asset weakness, end-to-end against the real registry.
//!
//! ```text
//! Revelation - Put The Necronomicon into play in your threat area, with 3
//!   horror on it. It cannot leave play while it has 1 or more horror on it.
//! Treat each [elder_sign] you reveal on a chaos token as a [auto_fail].
//! ```
//!
//! This file covers the Revelation reaching the board (#1090) and the elder
//! sign counted as an auto-fail (#1094). `glossary/Weakness.md`:
//!
//! > When an investigator draws a weakness with a player cardtype *(for
//! > example, an asset, an event, or a skill weakness)*, resolve any Revelation
//! > effects on the card, and add it to that investigator's hand.
//!
//! > If a weakness enters an investigator's hand in a manner that did not
//! > involve drawing the card, that investigator must resolve the card
//! > (including any Revelation abilities) as if he or she had just drawn it.
//!
//! And the 01009 ruling (<https://arkhamdb.com/card/01009>): *"If you add this
//! to your hand without drawing (e.g. via Research Librarian), the
//! **Revelation** ability still triggers."*
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::card_data::CardType;
use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, EnemyId, GameStateBuilder, InvestigatorId,
    LocationId, Owner, SkillKind, TokenModifiers, TokenResolution, UseKind,
};
use game_core::test_support::{self, horror, seat, uses, Seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

const NECRONOMICON: &str = "01009";
const OLD_BOOK: &str = "01031"; // Item. Tome. — searches the top 3
const LIBRARIAN: &str = "01032"; // searches the deck for a Tome asset
const GUTS: &str = "01089"; // a skill: filler that is neither weakness nor Tome
const DAISY: &str = "01002"; // [elder_sign] effect: +0. If you succeed, draw 1 card for each [[Tome]] you control.
const MEDICAL_TEXTS: &str = "01035"; // Item. Tome.
const SHRIVELLING: &str = "01060"; // its reveal clause lists [auto_fail], not [elder_sign]

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const ENEMY: EnemyId = EnemyId(100);

/// `investigator`'s turn, alone at `HERE`, with no enemies (so the Draw
/// action's attacks of opportunity are empty and it resolves synchronously).
fn board(investigator: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(investigator)
        .open_turn(INV)
        .session()
}

/// Take the `position`th eligible card of a deck search. Sent raw for the
/// reason `research_librarian.rs` gives: a deck search's options are cards in
/// a deck, with no board anchor a session step could name.
fn take_searched(position: u32) -> Action {
    Action::Player(PlayerAction::ResolveInput {
        response: InputResponse::PickSingle(OptionId(position)),
    })
}

/// The Necronomicon is in `INV`'s threat area with 3 horror, owned by its
/// bearer, and not in hand; it entered without any harm being dealt.
fn assert_entered_with_three_horror(session: &TestSession) {
    let inv = &session.state().investigators[&INV];
    assert!(
        !inv.hand.iter().any(|c| c.as_str() == NECRONOMICON),
        "a Revelation that puts the card into play skips the hand",
    );
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == NECRONOMICON)
        .expect("The Necronomicon enters its bearer's threat area");
    assert_eq!(
        placed.accumulated_horror, 3,
        "\"with 3 horror on it\" is placed as it enters",
    );
    assert_eq!(placed.owner, Owner::Investigator(INV));
    assert_event!(
        session.events(),
        Event::CardEnteredThreatArea { code, .. } if code.as_str() == NECRONOMICON
    );
    // The horror is placed, not dealt: no harm event fires, and the
    // investigator takes none.
    assert_no_event!(session.events(), Event::HorrorTaken { .. });
    assert_no_event!(session.events(), Event::DamageTaken { .. });
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}

/// A searched-for Necronomicon's Revelation resolves once the search has
/// finished: the deck shuffles first, then the card enters play.
fn assert_resolved_after_the_shuffle(session: &TestSession) {
    let events = session.events();
    let shuffled = events
        .iter()
        .position(|e| matches!(e, Event::DeckShuffled { .. }))
        .expect("the search shuffles the deck");
    let entered = events
        .iter()
        .position(|e| matches!(e, Event::CardEnteredThreatArea { .. }))
        .expect("The Necronomicon enters the threat area");
    assert!(
        shuffled < entered,
        "the Revelation resolves after the shuffle: {events:?}",
    );
}

#[test]
fn drawing_it_puts_it_into_the_threat_area_with_three_horror() {
    let session = board(seat(1).at(HERE).actions(3).deck([NECRONOMICON, GUTS, GUTS]))
        .take(&TurnAction::Draw { investigator: INV });

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    // The reveal announces the card's own type: an asset, not a treachery.
    assert_event!(
        session.events(),
        Event::CardRevealed { code, card_type: CardType::Asset, .. }
            if code.as_str() == NECRONOMICON
    );
}

/// Research Librarian 01032 (*"Search your deck for a Tome asset and add it to
/// your hand"*) is the case the 01009 ruling names: the Revelation fires, and
/// it resolves after the search has shuffled the deck.
#[test]
fn fetched_by_research_librarian_the_revelation_still_fires() {
    let session = board(
        seat(1)
            .at(HERE)
            .hand([LIBRARIAN])
            .deck([GUTS, NECRONOMICON, GUTS]),
    )
    .take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    });
    // The Librarian's EnteredPlay reaction: fire it. One eligible Tome, so
    // the search auto-takes it.
    let librarian = session.state().card(INV, LIBRARIAN);
    let session = session.pick(librarian);

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::CardSearchedToHand { code, .. } if code.as_str() == NECRONOMICON
    );
    assert_entered_with_three_horror(&session);
    assert_resolved_after_the_shuffle(&session);
}

/// Daisy Walker at `HERE` with two Tomes in play (Old Book of Lore, Medical
/// Texts), `in_threat_area` in her threat area, and only the elder sign in the
/// bag.
fn daisy_drawing_the_elder_sign(in_threat_area: &[&str]) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .with_token_modifiers(TokenModifiers::default())
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .as_card(DAISY)
                .at(HERE)
                .deck([GUTS, GUTS, GUTS, GUTS])
                .in_play([OLD_BOOK, MEDICAL_TEXTS])
                .threat_area(in_threat_area.iter().map(|&code| (code, horror(3)))),
        )
}

/// Daisy's Willpower 3 test against 3 on her open turn, committing nothing,
/// run to the end.
fn willpower_test(state: GameStateBuilder) -> TestSession {
    let session = state
        .open_turn(INV)
        .session()
        .test_skill(INV, SkillKind::Willpower, 3)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// *"Treat each `[elder_sign]` you reveal on a chaos token as a
/// `[auto_fail]`."* Willpower 3 + 0 against 3 would pass on Daisy's elder sign
/// and draw one card per Tome; counted as an auto-fail, the elder sign fails
/// the test, and her elder-sign ability never initiates, so she draws nothing.
#[test]
fn the_elder_sign_daisy_reveals_is_an_automatic_failure() {
    let session = willpower_test(daisy_drawing_the_elder_sign(&[NECRONOMICON]));
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::ElderSign,
            resolution: TokenResolution::AutoFail,
        }
    );
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_no_event!(session.events(), Event::CardsDrawn { .. });
    assert!(session.state().investigators[&INV].hand.is_empty());
}

/// The control on the same board: without The Necronomicon, the elder sign is
/// Daisy's, the test passes, and she draws one card per Tome.
#[test]
fn without_it_the_elder_sign_draws_one_card_per_tome() {
    let session = willpower_test(daisy_drawing_the_elder_sign(&[]));
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::ElderSign,
            resolution: TokenResolution::ElderSign,
        }
    );
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
    assert_event!(
        session.events(),
        Event::CardsDrawn { investigator, count: 2 } if *investigator == INV
    );
}

/// A reveal clause keyed on `[auto_fail]` fires on the counted elder sign.
/// Shrivelling 01060: *"If a `[skull]`, `[cultist]`, `[tablet]`,
/// `[elder_thing]`, or `[auto_fail]` symbol is revealed during this attack,
/// take 1 horror."* The elder sign is not in that list, but the elder sign
/// Daisy reveals counts as an `[auto_fail]`, so the attack costs her 1 horror.
#[test]
fn a_reveal_clause_keyed_on_auto_fail_fires_on_her_elder_sign() {
    let mut enemy = test_support::test_enemy(ENEMY.0, "Ghoul");
    enemy.fight = 3;
    enemy.max_health = 3;
    let state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .with_token_modifiers(TokenModifiers::default())
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .as_card(DAISY)
                .at(HERE)
                .in_play([(SHRIVELLING, uses(UseKind::Charges, 4))])
                .threat_area([(NECRONOMICON, horror(3))]),
        )
        .with_enemy_engaged(enemy, INV)
        .open_turn(INV)
        .build();
    let shrivelling = state.in_play(INV, SHRIVELLING);
    let session = TestSession::new(state, TEST_REGISTRIES)
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(shrivelling),
            address: AbilityAddress::Printed(0),
        })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_event_count!(
        session.events(),
        1,
        Event::HorrorTaken { investigator, amount: 1 } if *investigator == INV
    );
    assert_eq!(session.state().investigators[&INV].horror(), 1);
}

/// Old Book of Lore 01031 (*"searches the top 3 cards of his or her deck for a
/// card, draws it"*) fetching it fires the Revelation too.
#[test]
fn fetched_by_old_book_of_lore_the_revelation_still_fires() {
    let session =
        board(
            seat(1)
                .at(HERE)
                .in_play([OLD_BOOK])
                .deck([NECRONOMICON, GUTS, GUTS, GUTS]),
        );
    let book = session.state().in_play(INV, OLD_BOOK);
    // Three eligible cards in the top 3: the pick suspends. The Necronomicon
    // is the first of them, in deck order.
    let session = session
        .take(&TurnAction::ActivateAbility {
            investigator: INV,
            source: AbilitySource::InPlay(book),
            address: AbilityAddress::Printed(0),
        })
        .apply(take_searched(0));

    session.assert_at_turn_menu(INV);
    assert_event!(
        session.events(),
        Event::CardSearchedToHand { code, .. } if code.as_str() == NECRONOMICON
    );
    assert_entered_with_three_horror(&session);
    assert_resolved_after_the_shuffle(&session);
}
