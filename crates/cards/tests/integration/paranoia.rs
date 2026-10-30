//! Paranoia 01097, a basic weakness, from its printed card.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01097):
//! > **Revelation** - Discard all your resources.
//!
//! Ruling (<https://arkhamdb.com/card/01097>):
//! > During the Upkeep phase, you draw a card first, then gain a resource. So
//! > if you draw Paranoia, you immediately discard all resources, then gain 1.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{CardCode, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Zone};
use game_core::test_support::{self, seat, TestSession};
use game_core::{assert_event_sequence, assert_no_event};

const PARANOIA: &str = "01097";
const HOLY_ROSARY: &str = "01059"; // a real non-weakness asset, deck filler

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// The bearer at `HERE` with 5 resources and Paranoia on top of their deck, on
/// their open turn.
fn board() -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).actions(3).resources(5).deck([
            PARANOIA,
            HOLY_ROSARY,
            HOLY_ROSARY,
        ]))
        .open_turn(INV)
        .session()
}

/// *"Discard all your resources."* Drawn with the Draw action, it empties the
/// pool, then goes to its bearer's discard pile: a one-shot treachery, discarded
/// from the deck it was drawn from.
#[test]
fn drawn_with_the_draw_action_it_discards_every_resource() {
    let session = board().take(&TurnAction::Draw { investigator: INV });

    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.resources, 0);
    assert!(inv.hand.is_empty(), "Paranoia does not stay in hand");
    assert_eq!(inv.discard, vec![CardCode::new(PARANOIA)]);
    assert_no_event!(session.events(), Event::ResourcesPaid { .. });
    assert_event_sequence!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == PARANOIA,
        Event::ResourcesDiscarded { investigator, amount: 5 } if *investigator == INV,
        Event::CardDiscarded {
            code,
            from: Zone::Deck,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == PARANOIA && *to == INV,
    );
}

/// The ruling's upkeep order: Paranoia drawn at step 4.4 discards every
/// resource, then the upkeep's 1 resource is gained, so the bearer leaves the
/// Upkeep phase with exactly 1.
#[test]
fn drawn_in_upkeep_it_leaves_exactly_the_upkeep_resource() {
    let session = board().take(&TurnAction::EndTurn);

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.resources, 1);
    assert_eq!(inv.discard, vec![CardCode::new(PARANOIA)]);
    assert_event_sequence!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == PARANOIA,
        Event::ResourcesDiscarded { amount: 5, .. },
        Event::ResourcesGained { amount: 1, .. },
    );
}
