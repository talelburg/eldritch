use crate::state::{CardCode, GameStateBuilder, Location, LocationId};

#[test]
fn connect_wires_both_directions() {
    let mut state = GameStateBuilder::new()
        .with_location(Location::new(
            LocationId(1),
            CardCode("a".into()),
            "A",
            1,
            0,
        ))
        .with_location(Location::new(
            LocationId(2),
            CardCode("b".into()),
            "B",
            1,
            0,
        ))
        .build();
    state.connect(LocationId(1), LocationId(2));
    assert_eq!(
        state.locations[&LocationId(1)].connections,
        vec![LocationId(2)]
    );
    assert_eq!(
        state.locations[&LocationId(2)].connections,
        vec![LocationId(1)]
    );
}

#[test]
#[should_panic(expected = "connect: location LocationId(2) not found")]
fn connect_panics_on_a_location_that_is_not_in_play() {
    // A set-aside card has no `LocationId` at all now, so `connect` has
    // one zone to search. Layout wiring happens at entry instead.
    let mut state = GameStateBuilder::new()
        .with_location(Location::new(
            LocationId(1),
            CardCode("a".into()),
            "A",
            1,
            0,
        ))
        .build();
    state.connect(LocationId(2), LocationId(1));
}
