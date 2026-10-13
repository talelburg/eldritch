//! The [`Seat`](test_support::Seat) fixture (#1014): seating an investigator in
//! one expression through `with_seat`, observed only through the built state.
//!
//! Synthetic probes only (ADR 0016), local to this binary. `_seat_inv` models
//! the primitive "an investigator card with printed skills"; `_seat_asset` models
//! "an asset a player can play from hand", so the engine mints an instance id
//! after build. Neither stands in for a printed card.

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Skills};
use game_core::state::{CardCode, GameStateBuilder, InvestigatorId, LocationId};
use game_core::test_support::{self, seat, MockRegistry};

/// An investigator card printed 2/5/1/4, 7 health, 9 sanity.
const SEAT_INV: &str = "_seat_inv";
/// A 0-cost asset with no slot.
const ASSET: &str = "_seat_asset";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn metadata(code: &str, name: &str, kind: CardKind) -> CardMetadata {
    CardMetadata {
        code: code.to_owned(),
        name: name.to_owned(),
        traits: vec![],
        text: None,
        back_name: None,
        back_text: None,
        pack_code: "_mock".to_owned(),
        weakness: false,
        kind,
    }
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    MockRegistry::new()
        .with_card(metadata(
            SEAT_INV,
            "Seat Probe",
            CardKind::Investigator {
                class: Class::Neutral,
                skills: Skills {
                    willpower: 2,
                    intellect: 5,
                    combat: 1,
                    agility: 4,
                },
                health: 7,
                sanity: 9,
            },
        ))
        .with_card(metadata(
            ASSET,
            "Seat Asset",
            CardKind::Asset {
                class: Class::Neutral,
                cost: Some(0),
                xp: None,
                slots: vec![],
                health: None,
                sanity: None,
                skill_icons: SkillIcons::default(),
                is_fast: false,
                deck_limit: 2,
                uses: None,
                play_only_during_turn: false,
            },
        ))
        .install();
}

#[test]
fn a_bare_seat_is_the_stock_test_investigator() {
    let state = GameStateBuilder::new().with_seat(seat(1)).build();
    assert_eq!(state.investigators[&INV], test_support::test_investigator(1));
}

#[test]
fn as_card_seats_the_printed_investigator() {
    let state = GameStateBuilder::new()
        .with_seat(seat(1).as_card(SEAT_INV))
        .build();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.investigator_card.code.as_str(), SEAT_INV);
    assert_eq!(
        inv.skills,
        Skills {
            willpower: 2,
            intellect: 5,
            combat: 1,
            agility: 4,
        },
    );
    assert_eq!(inv.name, "Seat Probe");
    assert_eq!((inv.max_health(), inv.max_sanity()), (7, 9));
}

#[test]
fn skills_after_as_card_override_the_printed_ones() {
    let stated = Skills {
        willpower: 6,
        intellect: 6,
        combat: 6,
        agility: 6,
    };
    let state = GameStateBuilder::new()
        .with_seat(seat(1).as_card(SEAT_INV).skills(stated))
        .build();
    assert_eq!(state.investigators[&INV].skills, stated);
}

#[test]
fn harm_lands_on_the_investigator_card() {
    let state = GameStateBuilder::new()
        .with_seat(seat(1).harm(2, 3))
        .build();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.investigator_card.accumulated_damage, 2);
    assert_eq!(inv.investigator_card.accumulated_horror, 3);
    assert_eq!((inv.damage(), inv.horror()), (2, 3));
}

#[test]
fn the_scalar_and_zone_setters_fill_what_they_name() {
    let state = GameStateBuilder::new()
        .with_location(test_support::test_location(10, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .hand(["_h1", "_h2"])
                .deck(["_d1"])
                .discard(["_x1", "_x2", "_x3"])
                .resources(2)
                .clues(4)
                .actions(1),
        )
        .build();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.current_location, Some(HERE));
    assert_eq!(inv.hand, [CardCode::new("_h1"), CardCode::new("_h2")]);
    assert_eq!(inv.deck, [CardCode::new("_d1")]);
    assert_eq!(
        inv.discard,
        [
            CardCode::new("_x1"),
            CardCode::new("_x2"),
            CardCode::new("_x3")
        ],
    );
    assert_eq!(
        (inv.resources, inv.clues, inv.actions_remaining),
        (2, 4, 1)
    );
}

#[test]
#[should_panic(expected = "_seat_unknown")]
fn as_card_with_an_unknown_code_panics_naming_it() {
    let _ = seat(1).as_card("_seat_unknown");
}
