use super::*;
use crate::engine::Cx;
use crate::state::GameStateBuilder;
use crate::test_support::TEST_REGISTRIES;
use crate::{assert_event, assert_no_event, test_support};

#[test]
fn reengage_at_location_engages_sole_co_located_survivor() {
    let surv = InvestigatorId(2);
    let loc = LocationId(1);
    let survivor = test_support::seat(2).at(loc);
    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e
    };
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(survivor)
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([surv])
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), EnemyId(1));

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(surv));
    assert_event!(events, Event::EnemyEngaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == surv);
}

#[test]
fn reengage_at_location_no_co_located_investigator_leaves_unengaged() {
    let loc = LocationId(1);
    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e
    };
    let mut state = GameStateBuilder::default()
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([])
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), EnemyId(1));

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn reengage_at_location_tie_auto_picks_lead_first_in_turn_order() {
    // Two co-located survivors, Prey::Default → tie → engage turn_order-first (lead).
    let lead = InvestigatorId(2);
    let other = InvestigatorId(3);
    let loc = LocationId(1);
    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e.prey = Prey::Default;
        e
    };
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(test_support::seat(2).at(loc))
        .with_seat(test_support::seat(3).at(loc))
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([lead, other]) // lead first
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), EnemyId(1));

    assert_eq!(
        state.enemies[&EnemyId(1)].engaged_with,
        Some(lead),
        "tie engages the lead (turn_order-first)"
    );
    assert_event!(events, Event::EnemyEngaged { enemy, investigator }
        if *enemy == EnemyId(1) && *investigator == lead);
}

#[test]
fn reengage_at_location_exhausted_enemy_does_not_engage() {
    let surv = InvestigatorId(2);
    let loc = LocationId(1);
    let survivor = test_support::seat(2).at(loc);
    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = Some(loc);
        e.engaged_with = None;
        e.exhausted = true; // exhausted unengaged enemy does not engage (RR p.10)
        e
    };
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_seat(survivor)
        .with_location(test_support::test_location(1, "Study"))
        .with_enemy(enemy)
        .with_turn_order([surv])
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), EnemyId(1));

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

#[test]
fn reengage_at_location_enemy_without_location_is_noop() {
    let enemy = {
        let mut e = test_support::test_enemy(1, "Ghoul");
        e.current_location = None; // no location — must no-op
        e.engaged_with = None;
        e
    };
    let mut state = GameStateBuilder::default().with_enemy(enemy).build();
    let mut events = Vec::new();
    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), EnemyId(1));
    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, None);
    assert_no_event!(events, Event::EnemyEngaged { .. });
}

/// A `Prey::BearerOnly` enemy re-engaging (readying, relocation, elimination)
/// skips the turn-order-first lead for its bearer (#1125). `glossary/Prey.md`:
/// *"that enemy only moves towards and engages that investigator (as if it were
/// the only investigator in play), and ignores all other investigators while
/// moving and engaging."* That it engages no one beside only non-bearers is
/// driven through `apply` by `crates/cards/tests/silver_twilight_acolyte.rs`.
#[test]
fn reengage_at_location_bearer_only_picks_the_bearer_over_the_lead() {
    let loc = LocationId(1);
    let lead = InvestigatorId(2);
    let bearer = InvestigatorId(1);
    let enemy = {
        let mut e = test_support::test_enemy(1, "Weakness");
        e.prey = Prey::BearerOnly;
        e.owner = Owner::Investigator(bearer);
        e.current_location = Some(loc);
        e
    };
    let mut state = GameStateBuilder::default()
        .with_registries(TEST_REGISTRIES)
        .with_location(test_support::test_location(1, "Study"))
        .with_seat(test_support::seat(lead.0).at(loc))
        .with_seat(test_support::seat(bearer.0).at(loc))
        .with_enemy(enemy)
        .with_turn_order([lead, bearer])
        .build();
    let mut events = Vec::new();

    reengage_at_location(&mut Cx::for_test(&mut state, &mut events), EnemyId(1));

    assert_eq!(state.enemies[&EnemyId(1)].engaged_with, Some(bearer));
}
