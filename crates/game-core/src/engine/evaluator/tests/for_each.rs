//! `Effect::ForEach`: the walking frame.
//!
//! The body runs once per target, in player order, with the target bound as
//! `InvestigatorTarget::Each`.

use card_dsl::dsl::InvestigatorTargetSet;

use super::*;

/// A second location, away from [`HERE`].
const HALL: LocationId = LocationId(11);

/// Investigators 1, 2 and 3 seated in `order`, all at [`HERE`]. The lead is
/// the first of `order`, so player order is `order`.
fn three_at_here(order: [u32; 3]) -> GameState {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(HERE.0, "Study"))
        .with_seat(seat(1).at(HERE))
        .with_seat(seat(2).at(HERE))
        .with_seat(seat(3).at(HERE))
        .with_turn_order(order.map(InvestigatorId))
        .build()
}

/// Who gained resources, in the order the events landed.
fn gainers(events: &[Event]) -> Vec<InvestigatorId> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::ResourcesGained { investigator, .. } => Some(*investigator),
            _ => None,
        })
        .collect()
}

/// The body runs once per investigator, **in player order** rather than id
/// order, each time naming the iterated investigator through `Each` — while
/// `You` stays the controller.
#[test]
fn for_each_runs_the_body_once_per_investigator_in_player_order() {
    let mut state = three_at_here([3, 1, 2]);
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
        &dsl::for_each(
            InvestigatorTargetSet::All,
            dsl::gain_resources(InvestigatorTarget::Each, 1),
        ),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(
        gainers(&events),
        vec![InvestigatorId(3), InvestigatorId(1), InvestigatorId(2)],
    );
    assert!(state.continuations.is_empty(), "the walk pops when done");
}

/// An investigator who is no longer `Active` when the walk starts is not in
/// its set.
#[test]
fn for_each_leaves_out_an_investigator_who_has_resigned() {
    let mut state = three_at_here([1, 2, 3]);
    state
        .investigators
        .get_mut(&InvestigatorId(2))
        .expect("seated")
        .status = Status::Resigned;
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
        &dsl::for_each(
            InvestigatorTargetSet::All,
            dsl::gain_resources(InvestigatorTarget::Each, 1),
        ),
        ctx(1),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(gainers(&events), vec![InvestigatorId(1), InvestigatorId(3)]);
}

/// `AtControllerLocation` walks only the investigators standing where the
/// controller stands.
#[test]
fn for_each_at_controller_location_walks_only_the_co_located() {
    let mut state = three_at_here([1, 2, 3]);
    state
        .locations
        .insert(HALL, test_support::test_location(HALL.0, "Hall"));
    state
        .investigators
        .get_mut(&InvestigatorId(3))
        .expect("seated")
        .current_location = Some(HALL);
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
        &dsl::for_each(
            InvestigatorTargetSet::AtControllerLocation,
            dsl::gain_resources(InvestigatorTarget::Each, 1),
        ),
        ctx(2),
    );

    assert_eq!(outcome, EngineOutcome::Done);
    assert_eq!(gainers(&events), vec![InvestigatorId(1), InvestigatorId(2)]);
}

/// `Each` names nobody outside a `ForEach` body, so resolving it there is a
/// rejection rather than a silent fallback to the controller.
#[test]
fn each_outside_a_for_each_body_rejects() {
    let mut state = three_at_here([1, 2, 3]);
    let mut events = Vec::new();

    let outcome = run(
        &mut Cx::for_test(&mut state, &mut events),
        &dsl::gain_resources(InvestigatorTarget::Each, 1),
        ctx(1),
    );

    assert!(
        matches!(outcome, EngineOutcome::Rejected { .. }),
        "got {outcome:?}"
    );
}
