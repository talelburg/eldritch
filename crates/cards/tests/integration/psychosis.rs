//! Psychosis 01099, a basic weakness, from its printed card.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01099):
//! > **Revelation** - Add Psychosis to your threat area.
//! > **Forced** - After you take 1 or more horror: Take 1 direct damage.
//! > [action] [action]: Discard Psychosis.
//!
//! Rulings (<https://arkhamdb.com/card/01099>):
//! > Any investigator at the same location as the investigator with Psychosis
//! > in their threat area may trigger the [action][action] to discard
//! > Psychosis, as per the FAQ [V1.0, section 2.1].
//!
//! > Triggers even if you assign horror to an asset you control (you're still
//! > taking horror, even if you reassign it).
//!
//! Holy Rosary 01059 (sanity 2) soaks horror; Leather Coat 01072 (health 2)
//! could soak damage, which a direct deal must not offer it.

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::TurnAction;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, EnemyId, GameStateBuilder, InvestigatorId, LocationId,
    Owner,
};
use game_core::test_support::{enemy, location, seat, Either, Seat, SeatLookup as _, TestSession};

const PSYCHOSIS: &str = "01099";
const AGNES: &str = "01004";
const HOLY_ROSARY: &str = "01059";
const LEATHER_COAT: &str = "01072";

const INV: InvestigatorId = InvestigatorId(1);
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);
const ENEMY: EnemyId = EnemyId(100);
const HERE: LocationId = LocationId(1);

/// The bearer at `HERE` with Psychosis in their threat area.
fn bearer() -> Seat {
    seat(1).at(HERE).threat_area([PSYCHOSIS])
}

/// `bearer`'s open turn at `HERE`.
fn staged(bearer: impl Into<Seat<Either>>) -> GameStateBuilder {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(bearer)
        .open_turn(INV)
}

fn board(bearer: impl Into<Seat<Either>>) -> TestSession {
    staged(bearer).session()
}

/// *"**Revelation** - Add Psychosis to your threat area."* Drawn off the top
/// of the deck with the Draw action, it lands in the drawer's threat area.
#[test]
fn drawn_from_the_deck_it_lands_in_the_drawers_threat_area() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .deck([PSYCHOSIS, HOLY_ROSARY, HOLY_ROSARY]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::Draw { investigator: INV });
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == PSYCHOSIS)
        .expect("Psychosis is in the threat area");
    assert_eq!(placed.owner, Owner::Investigator(INV));
    assert!(!inv.hand.iter().any(|c| c.as_str() == PSYCHOSIS));
}

/// *"After you take 1 or more horror: Take 1 direct damage."*
#[test]
fn horror_on_the_investigator_deals_one_damage() {
    let session = board(bearer()).take_horror(INV, 1);
    session.assert_at_turn_menu(INV);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.horror(), 1);
    assert_eq!(inv.damage(), 1, "the forced took 1 damage");
}

/// *"Triggers even if you assign horror to an asset you control (you're still
/// taking horror, even if you reassign it)."* The Rosary soaks the only point,
/// and the forced still fires. Its damage is direct, so Leather Coat is never
/// offered: `glossary/Direct_Damage_Direct_Horror.md`, *"that damage or horror
/// must be assigned directly to the specified card, and cannot be assigned or
/// re-assigned elsewhere."*
#[test]
fn horror_soaked_entirely_by_an_asset_still_fires_with_no_soak_prompt() {
    let session = board(bearer().in_play([HOLY_ROSARY, LEATHER_COAT])).take_horror(INV, 1);
    let rosary = session.state().card(INV, HOLY_ROSARY);
    let session = session.pick(rosary);

    // No prompt for the direct damage: straight back to the menu.
    session.assert_at_turn_menu(INV);
    let state = session.state();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.horror(), 0, "the horror went to the Rosary");
    assert_eq!(inv.damage(), 1, "the direct damage is on the investigator");
    let coat = state.in_play(INV, LEATHER_COAT);
    let coat = inv
        .cards_in_play
        .iter()
        .find(|c| c.instance_id == coat)
        .expect("Leather Coat is in play");
    assert_eq!(coat.accumulated_damage, 0, "nothing reassigned to the coat");
}

/// A deal with no horror in it takes no horror: an attack dealing 1 damage
/// and 0 horror still announces the deal, and Psychosis stays quiet. The deck
/// keeps Upkeep's draw off an empty deck, whose horror would fire it.
#[test]
fn a_deal_with_no_horror_does_not_fire() {
    let session = staged(bearer().deck([HOLY_ROSARY]))
        .with_enemy_engaged(
            enemy(ENEMY).health(3).attack_damage(1).attack_horror(0),
            INV,
        )
        .session()
        .take(&TurnAction::EndTurn);
    let inv = &session.state().investigators[&INV];
    assert_eq!(inv.damage(), 1, "only the attack's damage");
    assert_eq!(inv.horror(), 0);
}

/// Taking damage is not taking horror: a 1-damage deal outside an attack does
/// not fire it either.
#[test]
fn taking_damage_does_not_fire() {
    let session = board(bearer()).take_damage(INV, 1);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].damage(), 1);
    assert_eq!(session.state().investigators[&INV].horror(), 0);
}

/// "You" is the investigator with Psychosis in their threat area
/// (`glossary/You_Your.md`). Horror a co-located neighbour takes is not theirs.
#[test]
fn horror_taken_by_someone_else_does_not_fire() {
    let session = staged(bearer())
        .with_seat(seat(2).at(HERE))
        .session()
        .take_horror(NEIGHBOUR, 1);
    assert_eq!(session.state().investigators[&NEIGHBOUR].horror(), 1);
    assert_eq!(session.state().investigators[&NEIGHBOUR].damage(), 0);
    assert_eq!(session.state().investigators[&INV].damage(), 0);
}

/// The `[action] [action]` ruling: a co-located neighbour discards it.
#[test]
fn a_co_located_investigator_discards_it_with_two_actions() {
    let session = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(location(HERE))
        .with_seat(bearer())
        .with_seat(seat(2).at(HERE))
        .open_turn(NEIGHBOUR)
        .session();
    let source = AbilitySource::InPlay(session.state().in_play(INV, PSYCHOSIS));
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
        vec![CardCode::new(PSYCHOSIS)]
    );
}

/// Agnes Baker 01004's *"\[reaction\] After 1 or more horror is placed on Agnes
/// Baker: Deal 1 damage to an enemy at your location."* is offered, and
/// resolves, before Psychosis's direct damage. Placing is nested inside
/// dealing, so the placement's `after` comes before the whole deal's. No
/// vendored source rules on this pair; it is the order the nesting gives.
#[test]
fn agnes_reaction_resolves_before_the_forced_damage() {
    let agnes = seat(1).as_card(AGNES).at(HERE).threat_area([PSYCHOSIS]);
    let session = staged(agnes)
        .with_enemy_engaged(
            enemy(ENEMY).health(3).attack_damage(0).attack_horror(1),
            INV,
        )
        .session()
        .take_horror(INV, 1);
    let card = session.state().investigators[&INV].card_anchor();
    assert_eq!(session.offered_at(&card), 1, "her reaction is offered");
    assert_eq!(
        session.state().investigators[&INV].damage(),
        0,
        "Psychosis has not fired yet"
    );

    let session = session.pick(card);
    session.assert_at_turn_menu(INV);
    assert_eq!(session.state().investigators[&INV].damage(), 1);
    let enemy_hit = session
        .events()
        .iter()
        .position(|e| matches!(e, Event::EnemyDamaged { enemy, .. } if *enemy == ENEMY))
        .expect("Agnes damaged the enemy");
    let psychosis_hit = session
        .events()
        .iter()
        .position(|e| matches!(e, Event::DamagePlacedOnInvestigator { investigator, .. } if *investigator == INV))
        .expect("Psychosis dealt the damage");
    assert!(enemy_hit < psychosis_hit, "events = {:?}", session.events());
}
