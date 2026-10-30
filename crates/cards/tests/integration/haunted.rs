//! Haunted 01098, a basic weakness, from its printed card.
//!
//! Card text (`data/arkhamdb-snapshot/pack/core/core.json`, 01098):
//! > **Revelation** - Add Haunted to your threat area.
//! > You get -1 to each of your skills.
//! > [action] [action]: Discard Haunted.
//!
//! Ruling (<https://arkhamdb.com/card/01098>):
//! > Any investigator at the same location as the investigator with Haunted in
//! > their threat area may trigger the [action][action] to discard Haunted, as
//! > per the FAQ [V1.0, section 2.1].

use cards::TEST_REGISTRIES;
use game_core::assert_event;
use game_core::engine::enumerate::TurnAction;
use game_core::engine::modified_value::{self, ModifiedQuantity, ReadContext};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, DiscardPile, GameState, GameStateBuilder,
    InvestigatorId, LocationId, ModifierTarget, Owner, SkillKind, Zone,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const HAUNTED: &str = "01098";
/// Holy Rosary 01059 — a non-weakness filler for the rest of the deck.
const HOLY_ROSARY: &str = "01059";

const BEARER: InvestigatorId = InvestigatorId(1);
const NEIGHBOUR: InvestigatorId = InvestigatorId(2);
const HERE: LocationId = LocationId(1);

const SKILLS: [SkillKind; 4] = [
    SkillKind::Willpower,
    SkillKind::Intellect,
    SkillKind::Combat,
    SkillKind::Agility,
];

/// `who`'s modified `skill`, read the way the engine reads it outside a test.
fn skill(state: &GameState, who: InvestigatorId, skill: SkillKind) -> i32 {
    modified_value::modified_value(
        state,
        TEST_REGISTRIES.cards,
        ModifierTarget::Investigator(who),
        ModifiedQuantity::Skill(skill),
        ReadContext::OutsideTest,
    )
    .total()
}

/// The bearer at `HERE` draws Haunted off the top of their deck with the Draw
/// action.
fn draw_haunted() -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .actions(3)
                .deck([HAUNTED, HOLY_ROSARY, HOLY_ROSARY]),
        )
        .open_turn(BEARER)
        .session()
        .take(&TurnAction::Draw {
            investigator: BEARER,
        })
}

/// Haunted already in the bearer's threat area, both investigators at `HERE`,
/// and `turn`'s turn open.
fn haunted_in_play(turn: InvestigatorId) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE).threat_area([HAUNTED]))
        .with_seat(seat(2).at(HERE))
        .open_turn(turn)
        .session()
}

/// *"**Revelation** - Add Haunted to your threat area."* Drawn from the top of
/// the deck, it is revealed into the drawer's threat area, owned by them.
#[test]
fn drawn_from_the_deck_it_lands_in_the_drawers_threat_area() {
    let session = draw_haunted();
    session.assert_at_turn_menu(BEARER);
    let inv = &session.state().investigators[&BEARER];
    assert!(!inv.hand.iter().any(|c| c.as_str() == HAUNTED));
    let placed = inv
        .threat_area
        .iter()
        .find(|c| c.code.as_str() == HAUNTED)
        .expect("Haunted is in the threat area");
    assert_eq!(placed.owner, Owner::Investigator(BEARER));
}

/// *"You get -1 to each of your skills."* The test investigator prints 3 in
/// every skill.
#[test]
fn each_of_the_bearers_skills_reads_one_lower() {
    let session = draw_haunted();
    for kind in SKILLS {
        assert_eq!(skill(session.state(), BEARER, kind), 2, "{kind:?}");
    }
}

/// *"you"* on a constant is the investigator with the card in their threat
/// area (`glossary/You_Your.md`), so a co-located neighbour keeps their 3s.
#[test]
fn a_neighbour_at_the_same_location_is_unaffected() {
    let session = haunted_in_play(BEARER);
    for kind in SKILLS {
        assert_eq!(skill(session.state(), NEIGHBOUR, kind), 3, "{kind:?}");
    }
}

/// The ruling: a co-located investigator may trigger the `[action][action]`.
/// The neighbour spends both actions, Haunted goes to its owner's discard
/// pile, and the bearer's skills are back to 3.
#[test]
fn a_co_located_investigator_discards_it_with_two_actions() {
    let session = haunted_in_play(NEIGHBOUR);
    let source = AbilitySource::InPlay(session.state().in_play(BEARER, HAUNTED));
    let session = session.take(&TurnAction::ActivateAbility {
        investigator: NEIGHBOUR,
        source,
        // Revelation, then the four skill constants, then the discard.
        address: AbilityAddress::Printed(5),
    });
    session.assert_at_turn_menu(NEIGHBOUR);
    let state = session.state();
    assert_eq!(state.investigators[&NEIGHBOUR].actions_remaining, 1);
    assert!(state.investigators[&BEARER].threat_area.is_empty());
    assert_eq!(
        state.investigators[&BEARER].discard,
        vec![CardCode::new(HAUNTED)]
    );
    assert_event!(
        session.events(),
        Event::CardDiscarded { code, from: Zone::ThreatArea, to: DiscardPile::Investigator(BEARER) }
            if code.as_str() == HAUNTED
    );
    for kind in SKILLS {
        assert_eq!(skill(state, BEARER, kind), 3, "{kind:?}");
    }
}
