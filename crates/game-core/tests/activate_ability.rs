//! End-to-end `PlayerAction::ActivateAbility` flow with a mock
//! `CardRegistry` covering one made-up activated ability.
//!
//! Lives at `crates/game-core/tests/` (a separate integration-test
//! binary, hence its own process and its own `OnceLock<CardRegistry>`)
//! so installing a mock registry here doesn't collide with game-core's
//! in-crate tests (which deliberately don't install one).
//!
//! No real card has a `Trigger::Activated` ability yet — `#38`
//! Hyperawareness will be the first. Until then, mock cards are the
//! only way to exercise the full activation flow.

use std::sync::OnceLock;

use game_core::card_data::CardMetadata;
use game_core::card_registry::CardRegistry;
use game_core::dsl::{
    activated, constant, gain_resources, modify, Ability, Cost, InvestigatorTarget, ModifierScope,
    Stat,
};
use game_core::engine::{apply, EngineOutcome};
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken, InvestigatorId, Phase, Status,
    TokenModifiers,
};
use game_core::test_support::{test_investigator, TestGame};
use game_core::{assert_event, assert_event_count, assert_no_event};
use game_core::{Action, PlayerAction};

/// Mock card code: `[fast] Spend 1 resource: gain 1 resource.` —
/// economically meaningless (pay 1 to gain 1) but exercises the
/// Resources cost + `[fast]` (0 action cost) + `GainResources` effect.
const FAST_RESOURCE_LOOP: &str = "MOCK1";

/// Mock card code: `[action] Exhaust: gain 1 resource.` — exercises
/// the `[action]` cost (1 action), Exhaust cost, and the source-
/// exhaust check that blocks re-activation.
const ACTION_EXHAUST_GAIN: &str = "MOCK2";

/// Mock card code: holds only a `Trigger::Constant` modifier, no
/// activated abilities. Used to test "`ability_index` points at non-
/// Activated trigger" rejection.
const CONSTANT_ONLY: &str = "MOCK3";

/// Mock card code: activated ability whose ONLY cost is the
/// `DiscardCardFromHand` stub. Used to test the TODO-reject path.
const DISCARD_COST_ABILITY: &str = "MOCK4";

fn mock_metadata_for(_: &CardCode) -> Option<&'static CardMetadata> {
    None
}

fn mock_abilities_for(code: &CardCode) -> Option<Vec<Ability>> {
    match code.as_str() {
        FAST_RESOURCE_LOOP => Some(vec![activated(
            0,
            vec![Cost::Resources(1)],
            gain_resources(InvestigatorTarget::Controller, 1),
        )]),
        ACTION_EXHAUST_GAIN => Some(vec![activated(
            1,
            vec![Cost::Exhaust],
            gain_resources(InvestigatorTarget::Controller, 1),
        )]),
        CONSTANT_ONLY => Some(vec![constant(modify(
            Stat::Willpower,
            1,
            ModifierScope::WhileInPlay,
        ))]),
        DISCARD_COST_ABILITY => Some(vec![activated(
            0,
            vec![Cost::DiscardCardFromHand],
            gain_resources(InvestigatorTarget::Controller, 1),
        )]),
        _ => None,
    }
}

fn install_mock_registry() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = game_core::card_registry::install(CardRegistry {
            metadata_for: mock_metadata_for,
            abilities_for: mock_abilities_for,
        });
    });
}

/// Build a state with one in-play instance of `code` (instance id 0),
/// in the Investigation phase, the controller active and Active,
/// 3 actions remaining, 5 starting resources (per the test fixture).
fn state_with_in_play(code: &str) -> (game_core::GameState, InvestigatorId, CardInstanceId) {
    install_mock_registry();

    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.cards_in_play
        .push(CardInPlay::enter_play(CardCode::new(code), instance_id));

    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        // Chaos bag content doesn't matter here — no skill test fires.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .build();
    (state, id, instance_id)
}

#[test]
fn fast_resource_loop_activates_and_resolves_effect() {
    // Pay 1 resource, gain 1 resource: net zero, but proves the full
    // cost-then-effect ordering and that a `[fast]` ability costs no
    // action.
    let (state, id, instance_id) = state_with_in_play(FAST_RESOURCE_LOOP);
    let actions_before = state.investigators[&id].actions_remaining;

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert_eq!(result.outcome, EngineOutcome::Done);
    let inv = &result.state.investigators[&id];
    assert_eq!(
        inv.actions_remaining, actions_before,
        "fast (action_cost = 0) doesn't spend an action",
    );
    // Net: 5 - 1 (paid) + 1 (gained) = 5.
    assert_eq!(inv.resources, 5);

    // Cost-then-effect ordering: ResourcesPaid → AbilityActivated →
    // ResourcesGained.
    assert_event_count!(
        result.events,
        1,
        Event::ResourcesPaid { investigator, amount: 1 } if *investigator == id
    );
    assert_event_count!(
        result.events,
        1,
        Event::AbilityActivated {
            investigator,
            ability_index: 0,
            code,
            ..
        } if *investigator == id && code.as_str() == FAST_RESOURCE_LOOP
    );
    assert_event_count!(
        result.events,
        1,
        Event::ResourcesGained { investigator, amount: 1 } if *investigator == id
    );
}

#[test]
fn action_exhaust_gain_exhausts_source_and_blocks_reactivation() {
    let (state, id, instance_id) = state_with_in_play(ACTION_EXHAUST_GAIN);
    let actions_before = state.investigators[&id].actions_remaining;

    let after_first = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert_eq!(after_first.outcome, EngineOutcome::Done);
    let inv = &after_first.state.investigators[&id];
    assert_eq!(inv.actions_remaining, actions_before - 1);
    assert_eq!(inv.resources, 5 + 1);
    assert!(
        inv.cards_in_play[0].exhausted,
        "source must exhaust after activation",
    );
    assert_event!(
        after_first.events,
        Event::CardExhausted { investigator, instance_id: iid, .. }
            if *investigator == id && *iid == instance_id
    );

    // Second activation: source is exhausted; Cost::Exhaust check
    // rejects without mutating state.
    let after_second = apply(
        after_first.state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert!(matches!(
        after_second.outcome,
        EngineOutcome::Rejected { .. }
    ));
    assert!(after_second.events.is_empty());
}

#[test]
fn insufficient_resources_reject_without_payment() {
    let (mut state, id, instance_id) = state_with_in_play(FAST_RESOURCE_LOOP);
    state.investigators.get_mut(&id).unwrap().resources = 0;

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
    // Wallet untouched.
    assert_eq!(result.state.investigators[&id].resources, 0);
}

#[test]
fn insufficient_actions_reject_action_cost_ability() {
    let (mut state, id, instance_id) = state_with_in_play(ACTION_EXHAUST_GAIN);
    state.investigators.get_mut(&id).unwrap().actions_remaining = 0;

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
    // Source still ready (not exhausted), resources untouched.
    let inv = &result.state.investigators[&id];
    assert!(!inv.cards_in_play[0].exhausted);
    assert_eq!(inv.resources, 5);
}

#[test]
fn ability_index_pointing_at_non_activated_trigger_rejects() {
    let (state, id, instance_id) = state_with_in_play(CONSTANT_ONLY);

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn ability_index_out_of_bounds_rejects() {
    let (state, id, instance_id) = state_with_in_play(FAST_RESOURCE_LOOP);

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 9,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn discard_card_from_hand_cost_rejects_with_todo() {
    let (state, id, instance_id) = state_with_in_play(DISCARD_COST_ABILITY);

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    // No partial mutation: source still ready, no events fired.
    assert!(result.events.is_empty());
    assert!(!result.state.investigators[&id].cards_in_play[0].exhausted);
}

#[test]
fn activating_with_defeated_status_doesnt_need_registry() {
    // Belt-and-suspenders: even with registry installed, the status
    // check rejects before the registry lookup runs.
    install_mock_registry();
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_investigator(1);
    inv.status = Status::Killed;
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(FAST_RESOURCE_LOOP),
        instance_id,
    ));

    let state = TestGame::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .build();

    let result = apply(
        state,
        Action::Player(PlayerAction::ActivateAbility {
            investigator: id,
            instance_id,
            ability_index: 0,
        }),
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::AbilityActivated { .. });
}
