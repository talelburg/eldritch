//! Daisy's Tote Bag 01008 against the real corpus: *"You have 2 additional
//! hand slots, which can only be used to hold \[\[Tome\]\] assets."* Plus the
//! threat-area half of `glossary/Slots.md`: *"Each asset in an investigator's
//! play area or threat area with a slot symbol is held in a slot of that
//! type."*
//!
//! Mirrors `asset_slots.rs`: the real registries and a one-investigator open
//! turn with plenty of resources and actions.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, horror, seat, Seat, SeatLookup as _, TestSession};

const TOTE_BAG: &str = "01008"; // slot-less; the grant
const NECRONOMICON: &str = "01009"; // Tome, single Hand, a weakness
const OLD_BOOK_OF_LORE: &str = "01031"; // Tome, single Hand
const MEDICAL_TEXTS: &str = "01035"; // Tome, single Hand
const MAGNIFYING_GLASS: &str = "01030"; // Tool, single Hand
const KNIFE: &str = "01086"; // Weapon, single Hand
const FLASHLIGHT: &str = "01087"; // Tool, single Hand

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(101);

/// A one-investigator open turn on `seat`, with plenty of resources and
/// actions.
fn session_with(seat: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat.at(HERE).resources(30).actions(8))
        .with_location(test_support::test_location(HERE.0, "Study"))
        .open_turn(INV)
        .session()
}

/// Play the first card in hand from the turn menu.
fn play(session: TestSession) -> TestSession {
    session.take(&TurnAction::PlayCard {
        investigator: INV,
        hand_index: 0,
    })
}

fn codes_in_play(session: &TestSession) -> Vec<&str> {
    session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .map(|c| c.code.as_str())
        .collect()
}

#[test]
fn four_hand_tomes_fit_with_the_tote_bag_in_play() {
    let mut session = session_with(seat(1).in_play([TOTE_BAG]).hand([
        OLD_BOOK_OF_LORE,
        OLD_BOOK_OF_LORE,
        MEDICAL_TEXTS,
        MEDICAL_TEXTS,
    ]));
    for _ in 0..4 {
        session = play(session);
        session.assert_at_turn_menu(INV);
    }
    assert_eq!(
        codes_in_play(&session),
        vec![
            TOTE_BAG,
            OLD_BOOK_OF_LORE,
            OLD_BOOK_OF_LORE,
            MEDICAL_TEXTS,
            MEDICAL_TEXTS
        ],
        "two default hand slots plus the Tote Bag's two hold four Tomes"
    );
    assert!(session.state().investigators[&INV].discard.is_empty());
}

#[test]
fn a_third_non_tome_hand_asset_forces_make_room_despite_the_tote_bag() {
    let session = session_with(
        seat(1)
            .in_play([TOTE_BAG, KNIFE, FLASHLIGHT])
            .hand([MAGNIFYING_GLASS]),
    );
    let knife = session.state().in_play(INV, KNIFE);
    let flashlight = session.state().in_play(INV, FLASHLIGHT);
    let tote_bag = session.state().in_play(INV, TOTE_BAG);

    let session = play(session);
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(knife)),
            session.offered_at(&OptionTarget::CardInstance(flashlight)),
            session.offered_at(&OptionTarget::CardInstance(tote_bag)),
        ),
        (1, 1, 0),
        "the Tote Bag's slots take only Tomes, so the two non-Tomes are the \
         candidates: {:?}",
        session.prompt()
    );

    let session = session.pick(OptionTarget::CardInstance(knife));
    session.assert_at_turn_menu(INV);
    assert_eq!(
        codes_in_play(&session),
        vec![TOTE_BAG, FLASHLIGHT, MAGNIFYING_GLASS]
    );
    assert_eq!(
        session.state().investigators[&INV].discard,
        vec![CardCode::new(KNIFE)]
    );
}

/// Two Tomes in the Tote Bag's slots and two non-Tomes in the default ones.
/// A third non-Tome can only displace a non-Tome: discarding a Tome frees a
/// slot the new asset cannot use, so no Tome is offered.
#[test]
fn a_non_tome_never_occupies_a_tote_bag_slot() {
    let session = session_with(
        seat(1)
            .in_play([TOTE_BAG, OLD_BOOK_OF_LORE, MEDICAL_TEXTS, KNIFE, FLASHLIGHT])
            .hand([MAGNIFYING_GLASS]),
    );
    let old_book = session.state().in_play(INV, OLD_BOOK_OF_LORE);
    let medical_texts = session.state().in_play(INV, MEDICAL_TEXTS);
    let knife = session.state().in_play(INV, KNIFE);
    let flashlight = session.state().in_play(INV, FLASHLIGHT);

    let session = play(session);
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(old_book)),
            session.offered_at(&OptionTarget::CardInstance(medical_texts)),
            session.offered_at(&OptionTarget::CardInstance(knife)),
            session.offered_at(&OptionTarget::CardInstance(flashlight)),
        ),
        (0, 0, 1, 1),
        "only the non-Tomes are make-room candidates: {:?}",
        session.prompt()
    );
}

/// The Necronomicon sits in the threat area with its Hand slot. With one hand
/// asset in play, a second one takes the investigator over the two default
/// hand slots, so one hand asset has to go.
#[test]
fn the_necronomicon_in_the_threat_area_counts_against_hand_capacity() {
    let session = session_with(
        seat(1)
            .in_play([KNIFE])
            .threat_area([NECRONOMICON])
            .hand([FLASHLIGHT]),
    );
    let knife = session.state().in_play(INV, KNIFE);

    let session = play(session);
    let session = session.pick(OptionTarget::CardInstance(knife));
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(codes_in_play(&session), vec![FLASHLIGHT]);
    assert_eq!(inv.discard, vec![CardCode::new(KNIFE)]);
    assert_eq!(
        inv.threat_area
            .iter()
            .map(|c| c.code.as_str())
            .collect::<Vec<_>>(),
        vec![NECRONOMICON]
    );
}

/// With horror on it, The Necronomicon is never a make-room candidate: *"It
/// cannot leave play while it has 1 or more horror on it."*, and
/// `glossary/Cannot.md`: *"The word "cannot" is absolute, and cannot be
/// countermanded by other abilities."* Crypt Chill 01167's ruling applies the
/// same reading to a discard the player chooses
/// (<https://arkhamdb.com/card/01167>): *"You cannot choose and discard The
/// Necronomicon or Baron Samedi because they 'cannot leave play'."*
///
/// Two hand assets beside it already over-fill the two hand slots, a layout
/// the engine never checks until the next asset enters. Playing a third
/// offers only the two play-area assets.
#[test]
fn make_room_never_offers_the_necronomicon_while_it_has_horror() {
    let session = session_with(
        seat(1)
            .in_play([KNIFE, FLASHLIGHT])
            .threat_area([(NECRONOMICON, horror(3))])
            .hand([MAGNIFYING_GLASS]),
    );
    let knife = session.state().in_play(INV, KNIFE);
    let flashlight = session.state().in_play(INV, FLASHLIGHT);
    let necronomicon = session.state().in_play(INV, NECRONOMICON);

    let session = play(session);
    assert_eq!(
        (
            session.offered_at(&OptionTarget::CardInstance(knife)),
            session.offered_at(&OptionTarget::CardInstance(flashlight)),
            session.offered_at(&OptionTarget::CardInstance(necronomicon)),
        ),
        (1, 1, 0),
        "only the assets that can leave play are candidates: {:?}",
        session.prompt()
    );
}
