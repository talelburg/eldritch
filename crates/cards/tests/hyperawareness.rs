//! End-to-end test for Hyperawareness (01034): the two `[fast]`
//! activated abilities record a `ThisSkillTest`-scoped modifier for
//! intellect (index 0) or agility (index 1).
//!
//! Printed text (`data/arkhamdb-snapshot/pack/core/core.json`):
//! *"\[fast\] Spend 1 resource: You get +1 \[intellect\] for this skill
//! test."* / *"\[fast\] Spend 1 resource: You get +1 \[agility\] for this
//! skill test."*
//!
//! Demonstrates the composition of three mechanisms with a real card:
//! - `Trigger::Activated { action_cost: 0 }` with no action designator, plus
//!   `Cost::Resources(1)` (#53)
//! - the `ModifierScope::ThisSkillTest` → `Lifetime::SkillTest` translation
//!   and its expiry (#102, #676)
//! - the skill-test resolution path that folds recorded rows in alongside
//!   the swept modifiers and the base skill (#92, #675).
//!
//! **Where the ability is bought matters.** A modifier bought "for this
//! skill test" needs a test to attach to, so it is offered — and accepted —
//! only at a player window *inside* a test; the card's ruling is what makes
//! the alternative unbounded (<https://arkhamdb.com/card/01034>: *"You can
//! use \[fast\] fast actions as many times as you want, as long as you can
//! pay the cost; there is no limit."*).

use game_core::engine::{ApplyResult, EngineOutcome, TurnAction};
use game_core::event::Event;
use game_core::state::{
    AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, GameState,
    InvestigatorId, Phase, SkillKind, TokenModifiers,
};
use game_core::test_support::{
    dispatch_turn_action_unchecked, drive_skill_test, perform_skill_test,
    perform_skill_test_no_commits, test_investigator, GameStateBuilder, TakeOneFastPlay,
};
use game_core::{assert_event, assert_no_event};

const HYPERAWARENESS: &str = "01034";

const INTELLECT_ABILITY: usize = 0;
const AGILITY_ABILITY: usize = 1;

#[ctor::ctor(unsafe)]
fn install_real_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Build a state with one Hyperawareness already in play (instance
/// id 0), the controller mid-investigation, 5 resources, a single
/// `Numeric(0)` chaos bag for predictable arithmetic. We seed in
/// play (rather than playing from hand) because the activation flow
/// is what this file tests; `PlayCard` is exercised elsewhere.
fn state_with_hyperawareness() -> (GameState, InvestigatorId, CardInstanceId) {
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(HYPERAWARENESS),
        instance_id,
    ));

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_turn_order([id])
        .with_investigator_turn(id)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, instance_id)
}

/// Run a `skill` test at `difficulty`, buying the ability at `option_index`
/// at the test's player window.
fn test_buying(
    state: GameState,
    id: InvestigatorId,
    skill: SkillKind,
    difficulty: i8,
    option_index: usize,
) -> ApplyResult {
    drive_skill_test(
        state,
        id,
        skill,
        difficulty,
        TakeOneFastPlay::at_index(option_index),
    )
}

#[test]
fn intellect_ability_buffs_the_intellect_test_it_is_bought_during() {
    // 3 intellect + 1 (the recorded row) + 0 (token) = 4 vs difficulty 4 →
    // succeed by 0. The base 3-intellect investigator would fail without it.
    let (state, id, _) = state_with_hyperawareness();
    let resources_before = state.investigators[&id].resources;

    let result = test_buying(state, id, SkillKind::Intellect, 4, INTELLECT_ABILITY);

    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    assert_eq!(
        result.state.investigators[&id].resources,
        resources_before - 1,
        "1 resource paid",
    );
    assert!(
        result.state.recorded_modifiers.is_empty(),
        "a test-scoped row expires with the test it names",
    );
}

#[test]
fn agility_ability_buffs_the_agility_test_it_is_bought_during() {
    // Same as above but the second ability buffs agility.
    let (state, id, _) = state_with_hyperawareness();

    let result = test_buying(state, id, SkillKind::Agility, 4, AGILITY_ABILITY);

    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Agility, margin: 0 }
            if *investigator == id
    );
}

#[test]
fn intellect_ability_does_not_buff_an_agility_test() {
    // The recorded row targets `Stat::Intellect`; an agility test ignores
    // it. 3 + 0 < 4 → fail by 1.
    let (state, id, _) = state_with_hyperawareness();

    let result = test_buying(state, id, SkillKind::Agility, 4, INTELLECT_ABILITY);

    assert_event!(
        result.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Agility, by: 1, .. }
            if *investigator == id
    );
}

/// The buff belongs to the test it was bought during, and to no other: the
/// next test is unbuffed (#676).
#[test]
fn the_buff_does_not_survive_into_the_next_test() {
    let (state, id, _) = state_with_hyperawareness();

    let first = test_buying(state, id, SkillKind::Intellect, 4, INTELLECT_ABILITY);
    assert_event!(
        first.events,
        Event::SkillTestSucceeded {
            skill: SkillKind::Intellect,
            ..
        }
    );

    let second = perform_skill_test_no_commits(first.state, id, SkillKind::Intellect, 4);
    assert_event!(
        second.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == id
    );
}

/// Bought at an open-turn menu with no test running, the modifier has no
/// test to attach to — so the activation is **refused**, and the test that
/// follows shows no bonus. Before #676 the resource was spent, nothing
/// visible happened, and the `+1` surfaced on whatever test came next.
#[test]
fn activation_with_no_test_in_flight_is_rejected_and_buffs_nothing_later() {
    let (state, id, instance_id) = state_with_hyperawareness();
    let resources_before = state.investigators[&id].resources;
    let action = TurnAction::ActivateAbility {
        investigator: id,
        source: AbilitySource::InPlay(instance_id),
        ability_index: 0,
    };

    assert!(
        !game_core::engine::legal_actions(&state).contains(&action),
        "the open-turn menu must not offer a buff that cannot be bought",
    );

    // The refusal comes from the RR initiation gate, which proves the effect
    // inert with no test to attach to — the same predicate that kept it off
    // the menu above. (The evaluator's own "no skill test in flight" rejection
    // sits behind it, for the non-activation paths; it is unit-tested in
    // `engine::evaluator`.)
    let rejected = dispatch_turn_action_unchecked(state, &action);
    let reason = match &rejected.outcome {
        EngineOutcome::Rejected { reason } => reason.to_string(),
        other => panic!("expected a rejection, got {other:?}"),
    };
    assert!(
        reason.contains(HYPERAWARENESS) && reason.contains("cannot be initiated"),
        "the reason must name the card and the problem; got: {reason}",
    );
    assert_no_event!(rejected.events, Event::AbilityActivated { .. });
    assert_eq!(
        rejected.state.investigators[&id].resources, resources_before,
        "a refused activation costs nothing",
    );
    assert!(rejected.state.recorded_modifiers.is_empty());

    // And the test that follows is unbuffed: 3 intellect vs difficulty 4.
    let after_test = perform_skill_test_no_commits(rejected.state, id, SkillKind::Intellect, 4);
    assert_event!(
        after_test.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == id
    );
}

#[test]
fn activation_rejects_when_controller_lacks_a_resource() {
    // Inside a test (where the ability is buyable at all), with an empty
    // wallet: the cost gate refuses and nothing is recorded.
    let (mut state, id, instance_id) = state_with_hyperawareness();
    state.investigators.get_mut(&id).unwrap().resources = 0;
    let started = perform_skill_test(state, id, SkillKind::Intellect, 4);

    let result = dispatch_turn_action_unchecked(
        started.state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            ability_index: 0,
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
    // No partial mutation: resources still 0, nothing recorded.
    assert_eq!(result.state.investigators[&id].resources, 0);
    assert!(result.state.recorded_modifiers.is_empty());
}

#[test]
fn buying_the_buff_costs_no_action_points() {
    // `[fast]` activation must not spend an action.
    let (state, id, _) = state_with_hyperawareness();
    let actions_before = state.investigators[&id].actions_remaining;

    let result = test_buying(state, id, SkillKind::Intellect, 4, INTELLECT_ABILITY);

    assert_eq!(
        result.state.investigators[&id].actions_remaining, actions_before,
        "a fast ability spends no action",
    );
    assert_event!(result.events, Event::AbilityActivated { .. });
}
