//! End-to-end test for Hyperawareness (01034): the two `[fast]`
//! activated abilities push a `ThisSkillTest`-scoped modifier for
//! intellect (index 0) or agility (index 1).
//!
//! Demonstrates the composition of three Phase-3 mechanisms with a
//! real card:
//! - `Trigger::Activated { action_cost: 0 }` + `Cost::Resources(1)` (#53)
//! - `ModifierScope::ThisSkillTest` push + drain (#102)
//! - The skill-test resolution path that sums pending modifiers
//!   alongside constants and the base skill (#92).

use std::sync::Once;

use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, Phase, SkillKind,
    TokenModifiers,
};
use game_core::test_support::{test_investigator, TestGame};
use game_core::{assert_event, Action, PlayerAction};

const HYPERAWARENESS: &str = "01034";

const INTELLECT_ABILITY: u8 = 0;
const AGILITY_ABILITY: u8 = 1;

fn install_real_registry() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = game_core::card_registry::install(cards::REGISTRY);
    });
}

/// Build a state with one Hyperawareness already in play (instance
/// id 0), the controller mid-investigation, 5 resources, a single
/// `Numeric(0)` chaos bag for predictable arithmetic. We seed in
/// play (rather than playing from hand) because the activation flow
/// is what this file tests; `PlayCard` is exercised elsewhere.
fn state_with_hyperawareness() -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    install_real_registry();

    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(HYPERAWARENESS),
        instance_id,
    ));

    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, instance_id)
}

#[test]
fn intellect_ability_buffs_an_intellect_test_at_difficulty_4() {
    // 3 intellect + 1 (Hyperawareness push) + 0 (token) = 4 vs
    // difficulty 4 → succeed by 0. The base 3-intellect investigator
    // would fail without the activation.
    let (state, id, instance_id) = state_with_hyperawareness();
    let resources_before = state.investigators[&id].resources;

    let after_activate = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: INTELLECT_ABILITY,
        }),
    );
    assert_eq!(after_activate.outcome, EngineOutcome::Done);
    assert_eq!(
        after_activate.state.investigators[&id].resources,
        resources_before - 1,
        "1 resource paid",
    );

    let after_test = apply(
        after_activate.state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Intellect,
            difficulty: 4,
        }),
    );
    assert_eq!(after_test.outcome, EngineOutcome::Done);
    assert_event!(
        after_test.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    assert!(
        after_test.state.pending_skill_modifiers.is_empty(),
        "ThisSkillTest push must drain after the test ends",
    );
}

#[test]
fn agility_ability_buffs_an_agility_test_at_difficulty_4() {
    // Same as above but ability_index = 1 buffs agility.
    let (state, id, instance_id) = state_with_hyperawareness();

    let after_activate = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: AGILITY_ABILITY,
        }),
    );
    assert_eq!(after_activate.outcome, EngineOutcome::Done);

    let after_test = apply(
        after_activate.state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Agility,
            difficulty: 4,
        }),
    );
    assert_eq!(after_test.outcome, EngineOutcome::Done);
    assert_event!(
        after_test.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Agility, margin: 0 }
            if *investigator == id
    );
}

#[test]
fn intellect_ability_does_not_buff_an_agility_test() {
    // The pushed modifier targets `Stat::Intellect`; an agility test
    // ignores it. 3 + 0 < 4 → fail by 1.
    let (state, id, instance_id) = state_with_hyperawareness();

    let after_activate = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: INTELLECT_ABILITY,
        }),
    );
    assert_eq!(after_activate.outcome, EngineOutcome::Done);

    let after_test = apply(
        after_activate.state,
        Action::Player(PlayerAction::PerformSkillTest {
            investigator: id,
            skill: SkillKind::Agility,
            difficulty: 4,
        }),
    );
    assert_eq!(after_test.outcome, EngineOutcome::Done);
    assert_event!(
        after_test.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Agility, by: 1, .. }
            if *investigator == id
    );
}

#[test]
fn activation_rejects_when_controller_lacks_a_resource() {
    let (mut state, id, instance_id) = state_with_hyperawareness();
    state.investigators.get_mut(&id).unwrap().resources = 0;

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: INTELLECT_ABILITY,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
    // No partial mutation: resources still 0, no pending modifier.
    assert_eq!(result.state.investigators[&id].resources, 0);
    assert!(result.state.pending_skill_modifiers.is_empty());
}

#[test]
fn both_abilities_cost_no_action_points() {
    // `[fast]` activation must not spend an action. Confirm both
    // abilities behave this way.
    let (state, id, instance_id) = state_with_hyperawareness();
    let actions_before = state.investigators[&id].actions_remaining;

    let after_intellect = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: INTELLECT_ABILITY,
        }),
    );
    assert_eq!(after_intellect.outcome, EngineOutcome::Done);
    assert_eq!(
        after_intellect.state.investigators[&id].actions_remaining,
        actions_before,
    );

    let after_agility = apply(
        after_intellect.state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: AGILITY_ABILITY,
        }),
    );
    assert_eq!(after_agility.outcome, EngineOutcome::Done);
    assert_eq!(
        after_agility.state.investigators[&id].actions_remaining,
        actions_before,
    );
}
