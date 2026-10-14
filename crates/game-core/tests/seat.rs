//! The [`Seat`](test_support::Seat) fixture (#1014): seating an investigator in
//! one expression through `with_seat`, observed only through the built state.
//!
//! Synthetic probes only (ADR 0016), local to this binary. `_seat_inv` models
//! the primitive "an investigator card with printed skills"; `_seat_asset` models
//! "an asset a player can play from hand", so the engine mints an instance id
//! after build. Neither stands in for a printed card.

use std::sync::LazyLock;

use card_dsl::card_data::{CardKind, CardMetadata, Class, SkillIcons, Skills};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, GameStateBuilder, InvestigatorId, LocationId, Owner,
    SkillKind, Status, UseKind,
};
use game_core::test_support::{self, clues, seat, uses, MockRegistry, SeatLookup as _};

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

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
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
            // Registered, so it is playable: the engine plays only cards it knows.
            .with_abilities(ASSET, Vec::new)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

#[test]
fn a_bare_seat_is_the_stock_test_investigator() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1))
        .build();
    assert_eq!(
        state.investigators[&INV],
        test_support::test_investigator(1)
    );
}

#[test]
fn as_card_seats_the_printed_investigator() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
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
    assert_eq!(
        (
            inv.max_health(registries().cards),
            inv.max_sanity(registries().cards)
        ),
        (7, 9)
    );
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
        .with_registries(registries())
        .with_seat(seat(1).as_card(SEAT_INV).skills(stated))
        .build();
    assert_eq!(state.investigators[&INV].skills, stated);
}

#[test]
fn skill_sets_one_skill_and_keeps_the_rest() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).skill(SkillKind::Combat, 5))
        .build();
    assert_eq!(
        state.investigators[&INV].skills,
        Skills {
            willpower: 3,
            intellect: 3,
            combat: 5,
            agility: 3,
        },
    );
}

#[test]
fn skill_after_as_card_overrides_that_printed_skill_alone() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).as_card(SEAT_INV).skill(SkillKind::Willpower, 6))
        .build();
    assert_eq!(
        state.investigators[&INV].skills,
        Skills {
            willpower: 6,
            intellect: 5,
            combat: 1,
            agility: 4,
        },
    );
}

#[test]
fn harm_lands_on_the_investigator_card() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
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
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Study"))
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
    assert_eq!((inv.resources, inv.clues, inv.actions_remaining), (2, 4, 1));
}

#[test]
fn in_play_cards_get_distinct_ids_and_are_found_by_code() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).in_play(["_p1", "_p2"]))
        .with_seat(seat(2).in_play(["_p3"]))
        .build();
    let ids = [
        state.in_play(INV, "_p1"),
        state.in_play(INV, "_p2"),
        state.in_play(InvestigatorId(2), "_p3"),
    ];
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
    assert_ne!(ids[1], ids[2]);
    let inv = &state.investigators[&INV];
    let p1 = &inv.cards_in_play[0];
    assert_eq!((p1.code.as_str(), p1.instance_id), ("_p1", ids[0]));
    assert_eq!(p1.owner, Owner::Investigator(INV));
    assert!(p1.uses.is_empty(), "a plain code enters with no uses");
}

#[test]
fn a_card_entry_carries_its_uses_and_clues() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .in_play([("_supplies", uses(UseKind::Supplies, 3))])
                .threat_area([("_clued", clues(2))]),
        )
        .build();
    let inv = &state.investigators[&INV];
    assert_eq!(inv.cards_in_play[0].uses[&UseKind::Supplies], 3);
    assert_eq!(inv.threat_area[0].clues, 2);
}

#[test]
fn threat_area_cards_land_there_and_the_lookup_finds_them() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).threat_area(["_treachery"]))
        .build();
    let inv = &state.investigators[&INV];
    assert!(inv.cards_in_play.is_empty());
    assert_eq!(inv.threat_area[0].code.as_str(), "_treachery");
    assert_eq!(
        inv.threat_area[0].owner,
        Owner::EncounterDeck,
        "a card with no printed player cardtype is the encounter deck's",
    );
    assert_eq!(
        state.in_play(INV, "_treachery"),
        inv.threat_area[0].instance_id
    );
}

#[test]
fn a_player_card_in_the_threat_area_is_its_bearers() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).threat_area([ASSET]))
        .build();
    assert_eq!(
        state.investigators[&INV].threat_area[0].owner,
        Owner::Investigator(INV)
    );
}

#[test]
fn in_play_as_keeps_its_id_and_minting_skips_past_it() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .in_play(["_minted"])
                .in_play_as(CardInstanceId(0), "_chosen"),
        )
        .build();
    assert_eq!(state.in_play(INV, "_chosen"), CardInstanceId(0));
    assert_ne!(state.in_play(INV, "_minted"), CardInstanceId(0));
}

#[test]
fn a_card_the_engine_mints_after_build_gets_a_fresh_id() {
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(
            seat(1)
                .at(HERE)
                .hand([ASSET])
                .in_play_as(CardInstanceId(0), "_chosen")
                .in_play(["_minted"]),
        )
        .open_turn(INV)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        });
    let state = session.state();
    let played = state.in_play(INV, ASSET);
    assert_ne!(played, state.in_play(INV, "_chosen"));
    assert_ne!(played, state.in_play(INV, "_minted"));
}

#[test]
fn a_hand_built_card_present_at_build_is_not_reminted() {
    let mut investigator = test_support::test_investigator(1);
    investigator.current_location = Some(HERE);
    investigator.hand = vec![CardCode::new(ASSET)];
    investigator.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("_hand_built"),
        CardInstanceId(0),
        Owner::Investigator(INV),
    ));
    let session = GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_investigator(investigator)
        .open_turn(INV)
        .session()
        .take(&TurnAction::PlayCard {
            investigator: INV,
            hand_index: 0,
        });
    assert_ne!(session.state().in_play(INV, ASSET), CardInstanceId(0));
}

#[test]
fn card_is_the_anchor_of_the_card_the_lookup_finds() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).in_play(["_first"]).threat_area(["_second"]))
        .build();
    assert_eq!(
        state.card(INV, "_second"),
        OptionTarget::CardInstance(state.in_play(INV, "_second")),
    );
    assert_ne!(state.card(INV, "_first"), state.card(INV, "_second"));
}

#[test]
#[should_panic(expected = "_absent")]
fn card_panics_as_the_lookup_does() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).in_play(["_present"]))
        .build();
    let _ = state.card(INV, "_absent");
}

/// A seat mints only past what was staged before it, so a card staged after
/// it with a hand-picked id can take an id the seat minted. `build` catches
/// the collision rather than hand back two cards with one id.
#[test]
#[should_panic(expected = "two staged cards share the instance id")]
fn a_hand_picked_id_staged_after_a_seat_that_minted_it_panics_at_build() {
    let mut later = test_support::test_investigator(2);
    later.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new("_later"),
        CardInstanceId(0),
        Owner::Investigator(InvestigatorId(2)),
    ));
    let _ = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).in_play(["_minted"]))
        .with_investigator(later)
        .build();
}

#[test]
#[should_panic(expected = "_absent")]
fn the_lookup_panics_when_no_card_matches() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).in_play(["_present"]))
        .build();
    let _ = state.in_play(INV, "_absent");
}

#[test]
#[should_panic(expected = "2 cards")]
fn the_lookup_panics_when_two_cards_match() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .in_play_as(CardInstanceId(5), "_twice")
                .in_play_as(CardInstanceId(6), "_twice"),
        )
        .build();
    let _ = state.in_play(INV, "_twice");
}

#[test]
#[should_panic(expected = "_seat_unknown")]
fn as_card_with_an_unknown_code_panics_naming_it() {
    let _ = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).as_card("_seat_unknown"));
}

/// `status` seats an investigator already eliminated.
#[test]
fn status_sets_the_investigators_status() {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).status(Status::Defeated))
        .build();
    assert_eq!(state.investigators[&INV].status, Status::Defeated);
}
