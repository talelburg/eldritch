//! Hypochondria 01100, a basic weakness, from its printed card; and its chain
//! with Psychosis 01099.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01100):
//! > **Revelation** - Add Hypochondria to your threat area.
//! > **Forced** - After you take 1 or more damage: Take 1 direct horror.
//! > [action] [action]: Discard Hypochondria.
//!
//! Rulings (<https://arkhamdb.com/card/01100>):
//! > Any investigator at the same location as the investigator with
//! > Hypochondria in their threat area may trigger the [action][action] to
//! > discard Hypochondria, as per the FAQ [V1.0, section 2.1].
//!
//! > Triggers even if you assign damage to an asset you control (you're still
//! > taking damage, even if you reassign it).
//!
//! Psychosis 01099: *"**Forced** - After you take 1 or more horror: Take 1
//! direct damage."*
//!
//! Leather Coat 01072 (health 2) soaks damage; Holy Rosary 01059 (sanity 2)
//! could soak horror, which a direct deal must not offer it.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, GameStateBuilder, InvestigatorId, LocationId, Owner,
    Status,
};
use game_core::test_support::{self, seat, Seat, SeatLookup as _, TestSession};

const HYPOCHONDRIA: &str = "01100";
const PSYCHOSIS: &str = "01099";
const HOLY_ROSARY: &str = "01059";
const LEATHER_COAT: &str = "01072";

const INV: InvestigatorId = InvestigatorId(1);
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(1);

/// The bearer at `HERE` with Hypochondria in their threat area.
fn bearer() -> Seat {
    seat(1).at(HERE).threat_area([HYPOCHONDRIA])
}

/// `bearer`'s open turn at `HERE`.
fn board(bearer: Seat) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(bearer)
        .open_turn(INV)
        .session()
}

/// *"**Revelation** - Add Hypochondria to your threat area."* Drawn off the
/// top of the deck with the Draw action, it lands in the drawer's threat area.
#[test]
fn drawn_from_the_deck_it_lands_in_the_drawers_threat_area() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .deck([HYPOCHONDRIA, HOLY_ROSARY, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == HYPOCHONDRIA)
        .expect("Hypochondria is in the threat area");
    assert_eq!(placed.owner, Owner::Investigator(INV));
}

/// *"After you take 1 or more damage: Take 1 direct horror."*
#[test]
fn damage_on_the_investigator_deals_one_horror() {
    let session = board(bearer()).take_damage(INV, 1);
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 1);
    assert_eq!(inv.horror(), 1, "the forced took 1 horror");
}

/// *"Triggers even if you assign damage to an asset you control"*: the coat
/// soaks the only point and the forced still fires, its direct horror never
/// offered to the Rosary.
#[test]
fn damage_soaked_entirely_by_an_asset_still_fires_with_no_soak_prompt() {
    let session = board(bearer().in_play([LEATHER_COAT, HOLY_ROSARY])).take_damage(INV, 1);
    let coat = session.state().card(INV, LEATHER_COAT);
    let session = session.pick(coat);

    session.assert_at_turn_menu(INV);
    let state = session.state();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.damage(), 0, "the damage went to the coat");
    assert_eq!(inv.horror(), 1, "the direct horror is on the investigator");
    let rosary = state.in_play(INV, HOLY_ROSARY);
    let rosary = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == rosary)
        .expect("Holy Rosary is in play");
    assert_eq!(rosary.accumulated_horror, 0, "nothing reassigned to it");
}

/// Taking horror is not taking damage.
#[test]
fn taking_horror_does_not_fire() {
    let session = board(bearer()).take_horror(INV, 1);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].damage(), 0);
}

/// The `[action] [action]` ruling: a co-located neighbour discards it.
#[test]
fn a_co_located_investigator_discards_it_with_two_actions() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(bearer())
        .with_seat(seat(2).at(HERE))
        .open_turn(NEIGHBOUR)
        .session();
    let source = AbilitySource::InPlay(session.state().in_play(INV, HYPOCHONDRIA));
    let session = session.take(&TurnAction::ActivateAbility {
        investigator: NEIGHBOUR,
        source,
        // Revelation, Forced, then the discard.
        address: AbilityAddress::Printed(2),
    });
    session.assert_at_turn_menu(NEIGHBOUR);
    let state = session.state();
    assert_eq!(state.investigators[&NEIGHBOUR].actions_remaining, 1);
    assert!(state.investigators[&INV].threat_area.is_empty());
    assert_eq!(
        state.investigators[&INV].discard,
        vec![CardCode::new(HYPOCHONDRIA)]
    );
}

/// With Psychosis too, each one's direct harm fires the other, and the chain
/// runs until the bearer is defeated. Starting from 1 horror, the test
/// investigator's 8 sanity is reached on the eighth horror, with 7 damage taken
/// in between; a defeated investigator's Psychosis fires no more. The turn then
/// passes to the second investigator.
#[test]
fn it_chains_with_psychosis_until_defeat() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::location(HERE))
        .with_seat(seat(1).at(HERE).threat_area([PSYCHOSIS, HYPOCHONDRIA]))
        .with_seat(seat(2).at(HERE))
        .with_seating([INV, NEIGHBOUR])
        .open_turn(INV)
        .session()
        .take_horror(INV, 1);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 8);
    assert_eq!(inv.damage(), 7);
    assert_ne!(inv.status, Status::Active);
    session.assert_at_turn_menu(NEIGHBOUR);
}
