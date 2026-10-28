//! The Necronomicon (John Dee Translation) 01009, Daisy Walker's signature
//! asset weakness, end-to-end against the real registry.
//!
//! ```text
//! Revelation - Put The Necronomicon into play in your threat area, with 3
//!   horror on it. It cannot leave play while it has 1 or more horror on it.
//! Treat each [elder_sign] you reveal on a chaos token as a [auto_fail].
//! [action]: Move 1 horror from The Necronomicon to Daisy Walker. Then, if The
//!   Necronomicon has no horror on it, discard it.
//! ```
//!
//! This file covers the Revelation reaching the board, making room in the hand
//! slots as it does, the elder sign counted as an auto-fail, the card holding
//! there while it has horror, and its `[action]` moving that horror to Daisy.
//! `glossary/Weakness.md`:
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
//! *"It cannot leave play while it has 1 or more horror on it."* is absolute:
//! `glossary/Cannot.md`, *"The word "cannot" is absolute, and cannot be
//! countermanded by other abilities."* Elimination still removes it, by step 0
//! of `glossary/Elimination.md`: *"For the purpose of resolving weakness cards,
//! the game has ended for the eliminated investigator. Trigger any “when the
//! game ends” abilities on each weakness the eliminated investigator owns that
//! is in play. Then, remove those weaknesses from the game."*
//!
//! Runs against `cards::TEST_REGISTRIES`.

use card_dsl::card_data::CardType;
use cards::TEST_REGISTRIES;
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::board::NotLeft;
use game_core::engine::enumerate::{PlaySource, TurnAction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, OptionId, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, ChaosBag, ChaosToken, DiscardPile, EliminationCause,
    EnemyId, GameState, GameStateBuilder, InvestigatorId, LocationId, Owner, SkillKind,
    TokenResolution, UseKind, Zone,
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
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
    assert_no_event!(session.events(), Event::DamagePlacedOnInvestigator { .. });
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
        source: PlaySource::Hand(0),
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

/// *"Treat each \[elder_sign\] you reveal on a chaos token as a
/// \[auto_fail\]."* Willpower 3 + 0 against 3 would pass on Daisy's elder sign
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
/// Shrivelling 01060: *"If a \[skull\], \[cultist\], \[tablet\],
/// \[elder_thing\], or \[auto_fail\] symbol is revealed during this attack,
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
        Event::HorrorPlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV
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

// ---- Making room as it enters the threat area ----
//
// `glossary/Put_into_Play.md`: *"Unless otherwise stated by the put into play
// ability, cards that enter play in this manner must do so in a play area
// that satisfies the standard game rules associated with playing or drawing
// (for encounter cards) that card."* The Necronomicon holds a hand slot from
// the threat area (`glossary/Slots.md`: *"Each asset in an investigator's
// play area or threat area with a slot symbol is held in a slot of that
// type."*), so entering over the limit makes room: *"the investigator must
// choose and discard other assets under his or her control simultaneously
// with the new asset entering the slot."*

const TOTE_BAG: &str = "01008"; // two Tome-only hand slots
const MAGNIFYING_GLASS: &str = "01030"; // Item. Tool. — single Hand
const FLASHLIGHT: &str = "01087"; // Item. Tool. — single Hand
const COVER_UP: &str = "01007"; // Task. — a weakness treachery, no slot symbol

/// Daisy draws `top` off her deck from the turn menu, with `in_play` already
/// in her play area.
fn daisy_draws(top: &str, in_play: &[&str]) -> TestSession {
    board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .actions(3)
            .in_play(in_play.iter().copied())
            .deck([top, GUTS, GUTS]),
    )
    .take(&TurnAction::Draw { investigator: INV })
}

fn codes_in_play(session: &TestSession) -> Vec<&str> {
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .map(|c| c.code.as_str())
        .collect()
}

#[test]
fn entering_over_the_hand_limit_asks_which_hand_asset_to_discard() {
    let session = daisy_draws(NECRONOMICON, &[MAGNIFYING_GLASS, FLASHLIGHT]);
    let glass = session.state().in_play(INV, MAGNIFYING_GLASS);
    let flashlight = session.state().in_play(INV, FLASHLIGHT);
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(glass)),
            session.offered_at(&OptionTarget::CardInstance(flashlight)),
        ),
        (1, 1),
        "expected a make-room prompt offering both hand assets, got {:?}",
        session.prompt(),
    );
    assert!(
        session.state().investigators[&INV].threat_area.is_empty(),
        "the card enters only once room is made",
    );

    let session = session.pick(OptionTarget::CardInstance(glass));

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_eq!(codes_in_play(&session), vec![FLASHLIGHT]);
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(MAGNIFYING_GLASS)],
    );
}

/// The same prompt pauses a Revelation under a deck search: Research Librarian
/// 01032 fetches it, the search shuffles, and the make-room prompt comes
/// before the card enters.
#[test]
fn fetched_over_the_hand_limit_it_makes_room_after_the_search() {
    let session = board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .resources(5)
            .in_play([MAGNIFYING_GLASS, FLASHLIGHT])
            .hand([LIBRARIAN])
            .deck([GUTS, NECRONOMICON, GUTS]),
    )
    .take(&TurnAction::PlayCard {
        investigator: INV,
        source: PlaySource::Hand(0),
    });
    let librarian = session.state().card(INV, LIBRARIAN);
    let session = session.pick(librarian);
    let flashlight = session.state().in_play(INV, FLASHLIGHT);
    assert_eq!(
        session.offered_at(&OptionTarget::CardInstance(flashlight)),
        1,
        "expected the make-room prompt, got {:?}",
        session.prompt(),
    );

    let session = session.pick(OptionTarget::CardInstance(flashlight));

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_resolved_after_the_shuffle(&session);
    assert_eq!(codes_in_play(&session), vec![MAGNIFYING_GLASS, LIBRARIAN]);
}

#[test]
fn a_free_hand_slot_takes_it_without_a_prompt() {
    let session = daisy_draws(NECRONOMICON, &[MAGNIFYING_GLASS]);

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_eq!(codes_in_play(&session), vec![MAGNIFYING_GLASS]);
    assert!(session.state().investigators[&INV].discard.is_empty());
}

/// Daisy's Tote Bag 01008: *"You have 2 additional hand slots, which can only
/// be used to hold \[\[Tome\]\] assets."* The Necronomicon is a Tome, so it
/// fits there with both default hand slots full.
#[test]
fn a_free_tote_bag_slot_takes_it_without_a_prompt() {
    let session = daisy_draws(NECRONOMICON, &[TOTE_BAG, MAGNIFYING_GLASS, FLASHLIGHT]);

    session.assert_at_turn_menu(INV);
    assert_entered_with_three_horror(&session);
    assert_eq!(
        codes_in_play(&session),
        vec![TOTE_BAG, MAGNIFYING_GLASS, FLASHLIGHT]
    );
    assert!(session.state().investigators[&INV].discard.is_empty());
}

/// A threat-area card with no slot symbol holds no slot, so it never makes
/// room, however full the hand slots are. Cover Up 01007: *"Revelation - Put
/// Cover Up into play in your threat area, with 3 clues on it."*
#[test]
fn a_slotless_threat_area_card_never_prompts() {
    let session = daisy_draws(COVER_UP, &[MAGNIFYING_GLASS, FLASHLIGHT]);

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    let cover_up = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == COVER_UP)
        .expect("Cover Up enters the threat area");
    assert_eq!(cover_up.clues, 3);
    assert_eq!(codes_in_play(&session), vec![MAGNIFYING_GLASS, FLASHLIGHT]);
    assert!(inv.discard.is_empty());
}

// ---- It cannot leave play while it has horror ----

/// Discard The Necronomicon through a native's discard-from-play exit, the way
/// a card-local "discard an asset" effect would. No corpus effect reaches an
/// asset in a threat area (Crypt Chill 01167 chooses among play-area assets),
/// so the exit is driven directly. Returns its answer, the state after, and the
/// events.
fn discard_the_necronomicon(session: &TestSession) -> (Result<(), NotLeft>, GameState, Vec<Event>) {
    let mut state = session.state().clone();
    let necronomicon = state.in_play(INV, NECRONOMICON);
    let mut left = Err(NotLeft::NotInPlay);
    let (outcome, events) = test_support::run_native(
        &mut state,
        TEST_REGISTRIES.cards,
        |cx, _| {
            left = cx.discard_from_play(necronomicon).map(|_| ());
            EngineOutcome::Done
        },
        &EvalContext::for_controller(INV),
    );
    assert!(matches!(outcome, EngineOutcome::Done));
    (left, state, events)
}

#[test]
fn discarding_it_while_it_has_horror_is_refused() {
    let session = board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .threat_area([(NECRONOMICON, horror(1))]),
    );

    let (left, state, events) = discard_the_necronomicon(&session);

    assert_eq!(left, Err(NotLeft::Refused));
    assert!(events.is_empty(), "nothing left play: {events:?}");
    let inv = &state.investigators[&INV];
    assert!(inv.discard.is_empty());
    let held = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == NECRONOMICON)
        .expect("it stays in the threat area");
    assert_eq!(held.accumulated_horror, 1, "with its horror still on it");
}

/// The prohibition lasts only *"while it has 1 or more horror on it"*.
#[test]
fn with_no_horror_on_it_it_can_be_discarded() {
    let session = board(seat(1).as_card(DAISY).at(HERE).threat_area([NECRONOMICON]));

    let (left, state, _) = discard_the_necronomicon(&session);

    assert_eq!(left, Ok(()));
    let inv = &state.investigators[&INV];
    assert!(inv.threat_area.is_empty());
    assert_eq!(inv.discard, vec![CardCode::new(NECRONOMICON)]);
}

/// Daisy's 5 damage defeats her (health 5). Elimination removes The
/// Necronomicon from the game with its 3 horror still on it: the game has
/// ended for her, and tokens leave *"simultaneously with the card leaving
/// play"* (`glossary/Leaves_Play.md`), so they never lift the prohibition first.
#[test]
fn eliminating_daisy_removes_it_while_it_has_horror() {
    let session = board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .threat_area([(NECRONOMICON, horror(3))]),
    )
    .take_damage(INV, 5);

    let inv = &session.state().investigators[&INV];
    assert!(inv.threat_area.is_empty(), "it left her threat area");
    assert!(
        inv.removed_from_game.contains(&CardCode::new(NECRONOMICON)),
        "removed from the game, to her pile: {:?}",
        inv.removed_from_game
    );
    assert_event!(
        session.events(),
        Event::CardRemovedFromGame { code, from: Zone::ThreatArea }
            if code.as_str() == NECRONOMICON
    );
}

// ---- Its [action] moves horror to Daisy ----

/// Activate The Necronomicon's `[action]`, its fourth printed ability (after
/// the Revelation, the cannot-leave-play constant and the token substitution),
/// and check the engine took it.
fn move_one_horror(session: TestSession) -> TestSession {
    let action = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, NECRONOMICON)),
        address: AbilityAddress::Printed(3),
    };
    let session = session.take(&action);
    assert_eq!(session.rejection(), None, "activating The Necronomicon");
    session
}

fn horror_on_the_necronomicon(state: &GameState) -> u8 {
    let necronomicon = state.in_play(INV, NECRONOMICON);
    state.investigators[&INV]
        .threat_area
        .iter()
        .find(|c| c.instance_id == necronomicon)
        .expect("The Necronomicon is in the threat area")
        .accumulated_horror
}

/// *"\[action\]: Move 1 horror from The Necronomicon to Daisy Walker."* A move,
/// so no harm event fires. The ruling (<https://arkhamdb.com/card/01009>):
/// *"Moving damage or horror is different from 'dealing' or 'taking' damage or
/// horror, so does not trigger effects or allow the player controlling Daisy
/// Walker to reassign the horror."* Daisy's restricted additional action pays
/// for it, since it is a Tome `[action]` ability, so her three standard actions
/// are untouched.
#[test]
fn its_action_moves_one_horror_to_daisy() {
    let session = board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .threat_area([(NECRONOMICON, horror(3))]),
    );
    let necronomicon = session.state().in_play(INV, NECRONOMICON);
    let daisy = session.state().investigators[&INV]
        .investigator_card
        .instance_id;

    let session = move_one_horror(session);

    session.assert_at_turn_menu(INV);
    assert_eq!(horror_on_the_necronomicon(session.state()), 2);
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    assert_event!(
        session.events(),
        Event::HorrorMoved { from, to, amount: 1 } if *from == necronomicon && *to == daisy
    );
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
    assert_no_event!(session.events(), Event::DamagePlacedOnInvestigator { .. });
    assert_eq!(
        session.state().investigators[&INV].actions_remaining,
        3,
        "her Tome-only additional action paid for it",
    );
    assert_no_event!(session.events(), Event::ActionsRemainingChanged { .. });
}

/// *"Then, if The Necronomicon has no horror on it, discard it."* The third
/// use empties it, and with no horror left it can leave play.
#[test]
fn the_third_use_discards_it() {
    let session = board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .threat_area([(NECRONOMICON, horror(3))]),
    );

    let session = move_one_horror(move_one_horror(session));
    assert_eq!(horror_on_the_necronomicon(session.state()), 1);
    assert_no_event!(session.events(), Event::CardDiscarded { .. });

    let session = move_one_horror(session);

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert!(inv.threat_area.is_empty(), "it left her threat area");
    assert_eq!(inv.discard, vec![CardCode::new(NECRONOMICON)]);
    assert_eq!(inv.horror(), 3, "all three horror moved to Daisy");
    assert_event!(
        session.events(),
        Event::CardDiscarded { code, from: Zone::ThreatArea, to: DiscardPile::Investigator(INV) }
            if code.as_str() == NECRONOMICON
    );
    assert_eq!(
        inv.actions_remaining, 1,
        "her additional action paid for the first use, standard actions for the other two",
    );
}

/// Daisy has 9 sanity. With 8 horror on her, the moved horror reaches it:
/// `glossary/Sanity_and_Horror.md`, *"If an investigator has horror on him or
/// her equal to or greater than his or her sanity, that investigator is
/// defeated."*
#[test]
fn a_move_that_reaches_her_sanity_defeats_her() {
    let session = board(
        seat(1)
            .as_card(DAISY)
            .at(HERE)
            .harm(0, 8)
            .threat_area([(NECRONOMICON, horror(3))]),
    );

    let session = move_one_horror(session);

    assert_eq!(session.state().investigators[&INV].horror(), 9);
    assert_event!(
        session.events(),
        Event::InvestigatorEliminated {
            investigator: INV,
            cause: EliminationCause::Horror
        }
    );
    assert_no_event!(session.events(), Event::HorrorPlacedOnInvestigator { .. });
}
