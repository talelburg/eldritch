use super::*;
use crate::test_support::BoardLookup as _;
use crate::test_support::TEST_REGISTRIES;

#[test]
fn place_assignment_accumulates_on_asset_and_investigator() {
    // Pre-construct an Assignment placing 1 damage + 1 horror on an
    // in-play asset and 1 damage on the investigator. TEST_INV = 8/8 and
    // the investigator damage is 1 < 8, so no defeat fires.
    // Asset defeat-on-overflow needs the real `cards` registry and is
    // covered by the EU5 integration test.

    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1).in_play(["01021"]))
        .build();
    let inst = state.in_play(id, "01021");
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);

    let mut asset_damage = BTreeMap::new();
    asset_damage.insert(inst, 1u8);
    let mut asset_horror = BTreeMap::new();
    asset_horror.insert(inst, 1u8);
    let assignment = Assignment {
        investigator_damage: 1,
        investigator_horror: 0,
        asset_damage,
        asset_horror,
    };

    place_assignment(&mut cx, id, &assignment);

    let card = &state.investigators[&id].cards_in_play[0];
    assert_eq!(card.accumulated_damage, 1, "asset soaked 1 damage");
    assert_eq!(card.accumulated_horror, 1, "asset soaked 1 horror");
    assert_eq!(
        state.investigators[&id].damage(),
        1,
        "investigator took overflow damage"
    );
    assert_event!(events, Event::DamagePlacedOnInvestigator { investigator, amount: 1 } if *investigator == id);
}

#[test]
fn damage_application_accumulates_on_the_investigator_card() {
    let id = InvestigatorId(1);
    let mut state = GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(1))
        .build();
    let mut events = Vec::new();
    let mut cx = Cx::for_test(&mut state, &mut events);
    let defeated = place_damage(&mut cx, id, 3);
    assert_eq!(
        state.investigators[&id]
            .investigator_card
            .accumulated_damage,
        3,
        "damage must accumulate on the investigator_card, not the legacy field"
    );
    assert_eq!(
        state.investigators[&id].damage(),
        3,
        "damage() accessor must read from investigator_card"
    );
    assert!(!defeated, "3 < 8 health — investigator not defeated");
}
