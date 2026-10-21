//! The deck-out penalty deals its horror through the one harm adapter, under
//! the real registry.
//!
//! `glossary/Drawing_Cards.md`: *"If an investigator with an empty investigator
//! deck needs to draw a card, that investigator shuffles his or her discard pile
//! back into his or her deck, then draws the card, and upon completion of the
//! entire draw takes one horror."*
//!
//! So the horror is dealt the way an enemy attack's is: a sanity-bearing asset
//! is offered the soak rather than taking it silently, and the horror finishes
//! resolving before a drawn weakness's Revelation does.
//!
//! Upkeep 4.4 runs one investigator at a time (Appendix II): *"In player order,
//! each investigator draws 1 card. Once those cards have been drawn, each
//! investigator gains 1 resource."* With `glossary/In_Player_Order.md`: *"the
//! lead investigator performs his or her part of the sequence first, followed
//! by the other players in clockwise order."*

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    GameStateBuilder, InvestigatorId, LocationId, Phase, UpkeepPhaseFrame, UpkeepResume,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

/// Cover Up 01007: *"Revelation - Put Cover Up into play in your threat area,
/// with 3 clues on it."* A persistent treachery weakness.
const COVER_UP: &str = "01007";
/// Holy Rosary 01059: sanity 2, no health. The sanity-bearing soaker.
const HOLY_ROSARY: &str = "01059";

const INV: InvestigatorId = InvestigatorId(1);
const INV2: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(101);

/// Investigator 1 takes the Draw action on an empty deck, with `discard` to
/// shuffle back and Holy Rosary in play.
fn draw_from_empty_deck(discard: &[&'static str]) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .in_play([HOLY_ROSARY])
                .discard(discard.iter().copied()),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV })
}

#[test]
fn deck_out_horror_offers_the_soak_to_a_sanity_bearing_asset() {
    let session = draw_from_empty_deck(&[HOLY_ROSARY]);
    let rosary = session.state().in_play(INV, HOLY_ROSARY);

    // The draw stops at the distribution prompt instead of auto-soaking.
    let me = session.state().investigators[&INV].card_anchor();
    assert_eq!(session.offered_at(&OptionTarget::CardInstance(rosary)), 1);
    assert_eq!(session.offered_at(&me), 1);
    assert_eq!(session.state().investigators[&INV].horror(), 0);

    // Take it on the investigator: the player's choice, not the soak-first default.
    let session = session.pick(me);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].horror(), 1);
    let rosary_card = session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == rosary)
        .expect("Holy Rosary still in play");
    assert_eq!(rosary_card.accumulated_horror, 0);
}

#[test]
fn deck_out_horror_lands_before_the_drawn_weakness_revelation_resolves() {
    // Cover Up is the only card to shuffle back, so the draw from the empty
    // deck pulls it. Rosary in play, so answer the soak on the investigator.
    let session = draw_from_empty_deck(&[COVER_UP]);
    let me = session.state().investigators[&INV].card_anchor();
    let session = session.pick(me);

    session.assert_at_turn_menu(INV);
    // The horror completes the draw before the Revelation resolves.
    assert_event_sequence!(
        session.events(),
        Event::HorrorTaken {
            investigator: INV,
            amount: 1
        },
        Event::CardEnteredThreatArea { code, .. } if code.as_str() == COVER_UP,
    );
}

/// The Upkeep 4.4 draw, the deck-out horror and the drawn Revelation for one
/// investigator all resolve before the next investigator draws; resources come
/// once every draw is done.
#[test]
fn upkeep_resolves_each_investigators_draw_completely_in_player_order() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).discard([COVER_UP]))
        .with_seat(seat(2).at(HERE).discard([COVER_UP]))
        .with_turn_order([INV, INV2])
        .with_phase(Phase::Upkeep)
        .with_phase_anchor(UpkeepPhaseFrame {
            resume: UpkeepResume::Begins,
        })
        .session();

    assert_event_sequence!(
        session.events(),
        Event::CardsDrawn {
            investigator: INV,
            ..
        },
        Event::HorrorTaken {
            investigator: INV,
            ..
        },
        Event::CardEnteredThreatArea {
            investigator: INV,
            ..
        },
        Event::CardsDrawn {
            investigator: INV2,
            ..
        },
        Event::HorrorTaken {
            investigator: INV2,
            ..
        },
        Event::CardEnteredThreatArea {
            investigator: INV2,
            ..
        },
        Event::ResourcesGained {
            investigator: INV,
            ..
        },
        Event::ResourcesGained {
            investigator: INV2,
            ..
        },
    );
    assert_eq!(session.state().phase, Phase::Mythos, "upkeep completed");
}

/// The upkeep deck-out horror offers the soak too, and the next investigator
/// does not draw until it is answered.
#[test]
fn upkeep_deck_out_soak_prompt_holds_the_next_investigators_draw() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .in_play([HOLY_ROSARY])
                .discard([HOLY_ROSARY]),
        )
        .with_seat(seat(2).at(HERE).deck([HOLY_ROSARY]))
        .with_turn_order([INV, INV2])
        .with_phase(Phase::Upkeep)
        .with_phase_anchor(UpkeepPhaseFrame {
            resume: UpkeepResume::Begins,
        })
        .session();

    let rosary = session.state().in_play(INV, HOLY_ROSARY);
    assert_eq!(session.offered_at(&OptionTarget::CardInstance(rosary)), 1);
    // Investigator 2 waits for investigator 1's draw to complete.
    assert_no_event!(
        session.events(),
        Event::CardsDrawn {
            investigator: INV2,
            ..
        }
    );

    let session = session.pick(OptionTarget::CardInstance(rosary));
    let rosary_card = session.state().investigators[&INV]
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == rosary)
        .expect("Holy Rosary still in play");
    assert_eq!(
        rosary_card.accumulated_horror, 1,
        "soaked by the player's choice"
    );
    assert_event!(
        session.events(),
        Event::CardsDrawn {
            investigator: INV2,
            ..
        }
    );
    assert_eq!(session.state().phase, Phase::Mythos, "upkeep completed");
}
