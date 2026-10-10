//! End-to-end ability-activation flow with a mock
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

use card_dsl::dsl::{
    self, Cost, Effect, EventPattern, EventTiming, IntExpr, InvestigatorTarget, LocationTarget,
    ModifierScope, Stat, UsageLimit, UsagePeriod,
};
use game_core::action::{Action, InputResponse, PlayerAction};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{self, EngineOutcome, OptionTarget};
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    GameState, GameStateBuilder, InvestigatorId, Lifetime, LocationId, Owner, Phase,
    RecordedModifierKind, SkillKind, Status, TokenModifiers,
};
use game_core::test_support::{self, MockRegistry, TakeOneFastPlay, TestSession};
use game_core::{assert_event, assert_event_count, assert_no_event};

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

/// Mock card code: Hyperawareness-shaped `[fast] Spend 1 resource:
/// you get +1 intellect for this skill test.` Exercises the
/// `ThisSkillTest` push path + accumulator drain across resolution.
const SKILL_BOOST: &str = "MOCK5";

/// Mock card code: `[fast] Gain 1 resource. (Limit once per round.)` — a
/// usage-limited activated ability, a primitive no corpus card prints yet
/// (#958).
const ONCE_PER_ROUND_GAIN: &str = "MOCK6";

/// Mock card code: `[fast] Gain 1 resource.`, gated by an eligibility tag
/// whose predicate holds only while the controller has a clue — an activated
/// ability whose condition lives outside its effect (#958, the #791 class).
const CLUE_GATED_GAIN: &str = "MOCK7";
const HAS_A_CLUE_TAG: &str = "_activate:has_a_clue";

/// Mock card code: `[fast] Discover 1 clue at your location. (Limit once per
/// round.)` — a usage-limited activated ability whose effect a `when`
/// interrupt can cancel.
const ONCE_PER_ROUND_DISCOVERY: &str = "MOCK8";

/// Mock card code: `[reaction] When an investigator would discover clues:
/// Cancel that discovery.` — the cancellation primitive (`Effect::Cancel` in
/// the `when` cell of a coordinator-owned condition), on no printed card.
const CANCEL_DISCOVERY: &str = "MOCK9";

fn has_a_clue(state: &GameState, ctx: &EvalContext) -> bool {
    state
        .investigators
        .get(&ctx.controller)
        .is_some_and(|inv| inv.clues > 0)
}

#[ctor::ctor(unsafe)]
fn install_mock_registry() {
    MockRegistry::new()
        .with_abilities(FAST_RESOURCE_LOOP, || {
            vec![dsl::activated(
                0,
                vec![Cost::Resources(1)],
                dsl::gain_resources(InvestigatorTarget::You, 1),
            )]
        })
        .with_abilities(ACTION_EXHAUST_GAIN, || {
            vec![dsl::activated(
                1,
                vec![Cost::Exhaust],
                dsl::gain_resources(InvestigatorTarget::You, 1),
            )]
        })
        .with_abilities(CONSTANT_ONLY, || {
            vec![dsl::constant(dsl::modify(
                Stat::Willpower,
                1,
                ModifierScope::WhileInPlay,
            ))]
        })
        .with_abilities(DISCARD_COST_ABILITY, || {
            vec![dsl::activated(
                0,
                vec![Cost::DiscardCardFromHand],
                dsl::gain_resources(InvestigatorTarget::You, 1),
            )]
        })
        .with_abilities(SKILL_BOOST, || {
            vec![dsl::activated(
                0,
                vec![Cost::Resources(1)],
                dsl::modify(Stat::Intellect, 1, ModifierScope::ThisSkillTest),
            )]
        })
        .with_abilities(ONCE_PER_ROUND_GAIN, || {
            vec![
                dsl::activated(0, vec![], dsl::gain_resources(InvestigatorTarget::You, 1))
                    .with_usage_limit(UsageLimit {
                        count: 1,
                        period: UsagePeriod::Round,
                    }),
            ]
        })
        .with_abilities(CLUE_GATED_GAIN, || {
            vec![
                dsl::activated(0, vec![], dsl::gain_resources(InvestigatorTarget::You, 1))
                    .with_eligibility(HAS_A_CLUE_TAG),
            ]
        })
        .with_native_eligibility(HAS_A_CLUE_TAG, has_a_clue)
        .with_abilities(ONCE_PER_ROUND_DISCOVERY, || {
            vec![dsl::activated(
                0,
                vec![],
                dsl::discover_clue(LocationTarget::YourLocation, 1),
            )
            .with_usage_limit(UsageLimit {
                count: 1,
                period: UsagePeriod::Round,
            })]
        })
        .with_abilities(CANCEL_DISCOVERY, || {
            vec![dsl::reaction_on_event(
                EventPattern::DiscoverClues,
                EventTiming::When,
                Effect::Cancel,
            )]
        })
        .install();
}

/// Build a state with one in-play instance of `code` (instance id 0),
/// in the Investigation phase, the controller active and Active,
/// 3 actions remaining, 5 starting resources (per the test fixture).
fn state_with_in_play(code: &str) -> (GameState, InvestigatorId, CardInstanceId) {
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(code),
        instance_id,
        Owner::Investigator(InvestigatorId(1)),
    ));

    let state = GameStateBuilder::new()
        .with_investigator(inv)
        .open_turn(id)
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

    let result = test_support::take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
    );
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
            address: AbilityAddress::Printed(0),
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

    let after_first = test_support::take_turn_action(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
    );
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
    let after_second = test_support::dispatch_turn_action_unchecked(
        after_first.state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
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

    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
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

    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
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

    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn ability_index_out_of_bounds_rejects() {
    let (state, id, instance_id) = state_with_in_play(FAST_RESOURCE_LOOP);

    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(9),
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
}

#[test]
fn discard_card_from_hand_cost_rejects_with_todo() {
    let (state, id, instance_id) = state_with_in_play(DISCARD_COST_ABILITY);

    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
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
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let mut inv = test_support::test_investigator(1);
    inv.status = Status::Defeated;
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(FAST_RESOURCE_LOOP),
        instance_id,
        Owner::Investigator(InvestigatorId(1)),
    ));

    let state = GameStateBuilder::new()
        .with_phase(Phase::Investigation)
        .with_investigator(inv)
        .with_active_investigator(id)
        .build();

    let result = test_support::dispatch_turn_action_unchecked(
        state,
        &TurnAction::ActivateAbility {
            investigator: id,
            source: AbilitySource::InPlay(instance_id),
            address: AbilityAddress::Printed(0),
        },
    );
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert_no_event!(result.events, Event::AbilityActivated { .. });
}

// ---- test-scoped recorded modifiers (#102, #676) -------------------

#[test]
fn a_test_scoped_modifier_buffs_the_test_it_was_bought_during() {
    // Activate SKILL_BOOST at the running test's player window: pay 1
    // resource, record +1 intellect for *this* test. Base 3 intellect + 1
    // exactly clears difficulty 4.
    let (state, id, _) = state_with_in_play(SKILL_BOOST);
    let resources_before = state.investigators[&id].resources;

    let result = test_support::drive_skill_test(
        state,
        id,
        SkillKind::Intellect,
        4,
        TakeOneFastPlay::at_index(0),
    );
    assert_event!(
        result.events,
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    assert_eq!(
        result.state.investigators[&id].resources,
        resources_before - 1,
        "the ability's cost was paid",
    );
    assert!(
        result.state.recorded_modifiers.is_empty(),
        "a test-scoped row expires with the test that owns it",
    );
}

#[test]
fn a_test_scoped_modifier_does_not_leak_into_a_second_test() {
    // Buy the boost inside one test, then run a second identical test
    // without buying it: 3 intellect vs difficulty 4 fails by 1.
    let (state, id, _) = state_with_in_play(SKILL_BOOST);

    let first = test_support::drive_skill_test(
        state,
        id,
        SkillKind::Intellect,
        4,
        TakeOneFastPlay::at_index(0),
    );
    let second =
        test_support::perform_skill_test_no_commits(first.state, id, SkillKind::Intellect, 4);
    assert_event!(
        second.events,
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == id
    );
}

#[test]
fn a_test_scoped_row_is_stamped_with_the_running_test_and_carries_its_source() {
    // Stop mid-test and read the row: it names the test in flight, and the
    // in-play instance whose ability recorded it (what limit-per-test logic
    // will key off).
    let (state, id, instance_id) = state_with_in_play(SKILL_BOOST);

    // Start the test, then take the offered fast play at its player window.
    let started = test_support::perform_skill_test(state, id, SkillKind::Intellect, 4);
    let EngineOutcome::AwaitingInput { request, .. } = &started.outcome else {
        panic!("expected the ST.1 fast window, got {:?}", started.outcome);
    };
    let option = request.options[0].id;
    let after_activate = engine::apply(
        started.state,
        Action::Player(PlayerAction::ResolveInput {
            response: InputResponse::PickSingle(option),
        }),
    );

    let in_flight = after_activate
        .state
        .current_skill_test()
        .expect("the test is still running")
        .id;
    assert_eq!(after_activate.state.recorded_modifiers.len(), 1);
    let row = &after_activate.state.recorded_modifiers[0];
    assert_eq!(row.investigator, id);
    assert_eq!(
        row.kind,
        RecordedModifierKind::Delta {
            stat: Stat::Intellect,
            delta: IntExpr::Lit(1),
        },
    );
    assert_eq!(row.lifetime, Lifetime::SkillTest(in_flight));
    assert_eq!(row.source, Some(instance_id));
}

/// A modifier bought "for this skill test" with no test running has no test
/// to attach to, so the activation is refused rather than banked (#676) —
/// and a client that submits it over the wire without consulting the menu
/// gets the same answer the menu gave.
#[test]
fn activating_a_test_scoped_modifier_outside_a_test_is_rejected() {
    let (state, id, instance_id) = state_with_in_play(SKILL_BOOST);
    let resources_before = state.investigators[&id].resources;
    let action = TurnAction::ActivateAbility {
        investigator: id,
        source: AbilitySource::InPlay(instance_id),
        address: AbilityAddress::Printed(0),
    };

    assert!(
        !enumerate::legal_actions(&state).contains(&action),
        "the open-turn menu must not offer an activation that would reject",
    );

    let result = test_support::dispatch_turn_action_unchecked(state, &action);
    assert!(matches!(result.outcome, EngineOutcome::Rejected { .. }));
    assert!(result.events.is_empty());
    // State unchanged by the rejection: no resource spent, nothing recorded.
    assert_eq!(result.state.investigators[&id].resources, resources_before);
    assert!(result.state.recorded_modifiers.is_empty());
}

/// A "Limit once per round" activated ability is offered once, refused once it
/// has been used, and offered again the next round (#958).
/// `glossary/Limits_and_Maximums.md`: *"Each instance of an ability with such a
/// limit may be initiated X times during the designated period."*
#[test]
fn a_once_per_round_activated_ability_is_offered_once_per_round() {
    let (state, id, instance_id) = state_with_in_play(ONCE_PER_ROUND_GAIN);
    let action = TurnAction::ActivateAbility {
        investigator: id,
        source: AbilitySource::InPlay(instance_id),
        address: AbilityAddress::Printed(0),
    };
    assert!(
        enumerate::legal_actions(&state).contains(&action),
        "an unused limited ability is offered",
    );

    let after_first = TestSession::new(state).take(&action);
    assert_eq!(after_first.state().investigators[&id].resources, 5 + 1);
    assert!(
        !enumerate::legal_actions(after_first.state()).contains(&action),
        "a once-per-round ability used this round is not offered again",
    );
    let refused =
        test_support::dispatch_turn_action_unchecked(after_first.state().clone(), &action);
    assert!(matches!(refused.outcome, EngineOutcome::Rejected { .. }));
    assert!(refused.events.is_empty());

    let mut next_round = after_first.state().clone();
    next_round.round += 1;
    assert!(
        enumerate::legal_actions(&next_round).contains(&action),
        "the limit resets when the round advances",
    );
}

/// A limited ability on a source the activator reaches without controlling it —
/// here a card attached to their location (#708) — records its use on that
/// card, wherever it sits, and is refused for the rest of the round.
#[test]
fn a_limited_ability_on_a_card_the_activator_does_not_control_counts_its_use() {
    let id = InvestigatorId(1);
    let instance_id = CardInstanceId(0);
    let location = LocationId(10);
    let mut loc = test_support::test_location(location.0, "Somewhere");
    loc.attachments.push(CardInPlay::enter_play(
        CardCode::new(ONCE_PER_ROUND_GAIN),
        instance_id,
        Owner::EncounterDeck,
    ));
    let state = GameStateBuilder::new()
        .with_investigator_at(test_support::test_investigator(1), location)
        .with_location(loc)
        .open_turn(id)
        .build();
    let action = TurnAction::ActivateAbility {
        investigator: id,
        source: AbilitySource::InPlay(instance_id),
        address: AbilityAddress::Printed(0),
    };

    let after_first = TestSession::new(state).take(&action);
    assert_eq!(after_first.state().investigators[&id].resources, 5 + 1);
    assert!(
        !enumerate::legal_actions(after_first.state()).contains(&action),
        "the use is counted against the attached card",
    );
}

/// An activated ability whose eligibility tag is false is not offered, and a
/// submission that skips the menu is refused; once the condition holds it is
/// offered (#958). The activation side of the bug class #791 closed for forced
/// abilities.
#[test]
fn a_tagged_activated_ability_is_not_offered_while_its_condition_is_false() {
    let (state, id, instance_id) = state_with_in_play(CLUE_GATED_GAIN);
    assert_eq!(state.investigators[&id].clues, 0);
    let action = TurnAction::ActivateAbility {
        investigator: id,
        source: AbilitySource::InPlay(instance_id),
        address: AbilityAddress::Printed(0),
    };
    assert!(
        !enumerate::legal_actions(&state).contains(&action),
        "a false eligibility condition keeps the ability off the menu",
    );
    let refused = test_support::dispatch_turn_action_unchecked(state.clone(), &action);
    assert!(matches!(refused.outcome, EngineOutcome::Rejected { .. }));
    assert!(refused.events.is_empty());

    let mut with_a_clue = state;
    with_a_clue.investigators.get_mut(&id).unwrap().clues = 1;
    assert!(
        enumerate::legal_actions(&with_a_clue).contains(&action),
        "the ability is offered once its condition holds",
    );
}

/// A limited ability whose effect is cancelled has still been initiated, so the
/// use counts: `glossary/Limits_and_Maximums.md`, *"If the effects of a card or
/// ability with a limit or maximum are canceled, it is still counted against
/// the limit/maximum, because the ability has been initiated."* The
/// once-per-round discovery is activated, an interrupt cancels the discovery it
/// would make, and the ability is not offered again this round.
#[test]
fn a_cancelled_use_of_a_limited_activated_ability_still_counts() {
    let id = InvestigatorId(1);
    let ability_card = CardInstanceId(0);
    let interrupt_card = CardInstanceId(1);
    let location = LocationId(10);
    let mut inv = test_support::test_investigator(1);
    inv.cards_in_play.push(CardInPlay::enter_play(
        CardCode::new(ONCE_PER_ROUND_DISCOVERY),
        ability_card,
        Owner::Investigator(InvestigatorId(1)),
    ));
    inv.threat_area.push(CardInPlay::enter_play(
        CardCode::new(CANCEL_DISCOVERY),
        interrupt_card,
        Owner::EncounterDeck,
    ));
    let mut loc = test_support::test_location(location.0, "Study");
    loc.clues = 2;
    let state = GameStateBuilder::new()
        .with_investigator_at(inv, location)
        .with_location(loc)
        .open_turn(id)
        .build();
    let action = TurnAction::ActivateAbility {
        investigator: id,
        source: AbilitySource::InPlay(ability_card),
        address: AbilityAddress::Printed(0),
    };

    let interrupted = TestSession::new(state).take(&action);
    let cancelled = interrupted.pick(OptionTarget::CardInstance(interrupt_card));

    assert_eq!(
        cancelled.state().investigators[&id].clues,
        0,
        "the discovery was cancelled",
    );
    assert_eq!(cancelled.state().locations[&location].clues, 2);
    assert!(
        !enumerate::legal_actions(cancelled.state()).contains(&action),
        "the cancelled use counts against the once-per-round limit",
    );
}
