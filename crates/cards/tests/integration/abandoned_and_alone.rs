//! Abandoned and Alone 01015, Wendy Adams' signature weakness, from its
//! printed card.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01015):
//! > **Revelation** - Take 2 direct horror and remove all cards in your discard
//! > pile from the game.
//!
//! Ruling (<https://arkhamdb.com/card/01015>):
//! > Remove cards from the game for the duration of the current scenario only.
//! > They're still part of your deck during this campaign.
//!
//! Holy Rosary 01059 (sanity 2) could soak horror, which a direct deal must not
//! offer it.
//!
//! Psychosis 01099 reacts to the horror: *"**Forced** - After you take 1 or more
//! horror: Take 1 direct damage."*

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    CardCode, DiscardPile, GameStateBuilder, InvestigatorId, LocationId, Status, Zone,
};
use game_core::test_support::{self, seat, BoardLookup as _};
use game_core::{assert_event, assert_event_sequence, assert_no_event};

const ABANDONED_AND_ALONE: &str = "01015";
const HOLY_ROSARY: &str = "01059";
const GUTS: &str = "01089";
const LUCKY: &str = "01080";
const PSYCHOSIS: &str = "01099";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Drawn with the Draw action, with the Rosary in play and two cards already in
/// the discard pile: the 2 horror go straight onto the investigator with no
/// soak offered (`glossary/Direct_Damage_Direct_Horror.md`: *"that damage or
/// horror must be assigned directly to the specified card, and cannot be
/// assigned or re-assigned elsewhere."*), the old discard pile leaves the game,
/// and Abandoned and Alone, discarded only once its Revelation is done, is
/// alone in the pile.
#[test]
fn drawn_it_takes_two_direct_horror_and_removes_the_discard_pile() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .in_play([HOLY_ROSARY])
                .discard([GUTS, LUCKY])
                .deck([ABANDONED_AND_ALONE, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    // No soak prompt: straight back to the menu.
    session.assert_at_turn_menu(INV);
    let state = session.state();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.horror(), 2, "both horror are on the investigator");
    let rosary = state.in_play(INV, HOLY_ROSARY);
    let rosary = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == rosary)
        .expect("the Rosary is in play");
    assert_eq!(rosary.accumulated_horror, 0, "the Rosary soaked nothing");
    assert_eq!(
        inv.removed_from_game,
        vec![CardCode::new(GUTS), CardCode::new(LUCKY)]
    );
    assert_eq!(inv.discard, vec![CardCode::new(ABANDONED_AND_ALONE)]);
    assert_event!(
        session.events(),
        Event::DiscardPileRemovedFromGame { investigator, cards }
            if *investigator == INV && cards.len() == 2
    );
    assert_event_sequence!(
        session.events(),
        Event::CardRevealed { code, .. } if code.as_str() == ABANDONED_AND_ALONE,
        Event::HorrorPlacedOnInvestigator { .. },
        Event::DiscardPileRemovedFromGame { .. },
        Event::CardDiscarded {
            code,
            from: Zone::Deck,
            to: DiscardPile::Investigator(to),
        } if code.as_str() == ABANDONED_AND_ALONE && *to == INV,
    );
}

/// **Its own horror can defeat its bearer** (ADR 0002). With 6 horror
/// on a sanity of 8, the 2 direct horror defeat the bearer while the card is
/// still resolving. Elimination removes the discard pile from the game with the
/// rest of their cards, and the card itself with them, since it is in limbo;
/// the removal step then finds nothing left to remove.
#[test]
fn horror_that_defeats_the_bearer_leaves_nothing_in_their_discard_pile() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .harm(0, 6)
                .discard([GUTS])
                .deck([ABANDONED_AND_ALONE, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.status, Status::Defeated);
    assert!(inv.discard.is_empty());
    let mut removed = inv.removed_from_game.clone();
    removed.sort();
    assert_eq!(
        removed,
        vec![
            CardCode::new(ABANDONED_AND_ALONE),
            CardCode::new(HOLY_ROSARY),
            CardCode::new(GUTS),
        ]
    );
    assert_no_event!(session.events(), Event::DiscardPileRemovedFromGame { .. });
    assert_no_event!(session.events(), Event::CardDiscarded { .. });
}

/// Both aspects of the Revelation resolve before an "after" ability reacts to
/// the first: `glossary/Effects.md`, *"All aspects of an effect have timing
/// priority over all "after..." triggering conditions that might arise as a
/// consequence of that effect."* So the discard pile leaves the game before
/// Psychosis deals its damage.
#[test]
fn the_discard_pile_is_removed_before_psychosis_reacts_to_the_horror() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .threat_area([PSYCHOSIS])
                .discard([GUTS])
                .deck([ABANDONED_AND_ALONE, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });

    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::HorrorPlacedOnInvestigator { investigator, amount: 2 } if *investigator == INV,
        Event::DiscardPileRemovedFromGame { investigator, .. } if *investigator == INV,
        Event::DamagePlacedOnInvestigator { investigator, amount: 1 } if *investigator == INV,
    );
}
