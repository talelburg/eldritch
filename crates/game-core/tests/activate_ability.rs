//! End-to-end ability-activation flow with a mock
//! `CardRegistry` covering one made-up activated ability.
//!
//! Lives at `crates/game-core/tests/` (a separate integration-test
//! binary, hence its own process and its own `MockRegistry`)
//! so building a mock registry here doesn't collide with game-core's
//! in-crate tests (which name game-core's test registry instead).
//!
//! Mock cards exercise the activation flow's shapes without depending on
//! a printed card.

use std::sync::LazyLock;

use card_dsl::dsl::{
    self, Cost, Effect, EventPattern, EventTiming, IntExpr, InvestigatorTarget, LocationTarget,
    ModifierScope, Stat, UsageLimit, UsagePeriod,
};
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, CardCode, CardInPlay, CardInstanceId, ChaosBag, ChaosToken,
    GameState, GameStateBuilder, InvestigatorId, Lifetime, LocationId, Owner, RecordedModifierKind,
    SkillKind, Status,
};
use game_core::test_support::{location, seat, MockRegistry, Seat, SeatLookup as _, TestSession};
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

/// Mock card code: `[fast] Choose and discard 1 card from your hand: gain 1
/// resource.` Its payment is `hand_discard_cost.rs`'s subject; here it is
/// refused with nothing in hand to pay with.
const DISCARD_COST_ABILITY: &str = "MOCK4";

/// Mock card code: Hyperawareness-shaped `[fast] Spend 1 resource:
/// you get +1 intellect for this skill test.` Exercises the
/// `ThisSkillTest` push path + accumulator drain across resolution.
const SKILL_BOOST: &str = "MOCK5";

/// Mock card code: `[fast] Gain 1 resource. (Limit once per round.)` — a
/// usage-limited activated ability, a primitive no corpus card prints yet.
const ONCE_PER_ROUND_GAIN: &str = "MOCK6";

/// Mock card code: `[fast] Gain 1 resource.`, gated by a native eligibility
/// condition whose predicate holds only while the controller has a clue — an activated
/// ability whose condition lives outside its effect.
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

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
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
                        .with_eligibility(dsl::native_condition(HAS_A_CLUE_TAG)),
                ]
            })
            .with_native_condition(HAS_A_CLUE_TAG, has_a_clue)
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
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

const INV: InvestigatorId = InvestigatorId(1);
/// The investigator's location, for the tests that need one.
const HERE: LocationId = LocationId(10);

/// `seated` with one in-play instance of `code`, on its open turn: 3 actions
/// remaining, 5 starting resources (per the test fixture). Returns the built
/// state, the investigator and the instance id of `code`.
fn state_seated(seated: Seat, code: &str) -> (GameState, InvestigatorId, CardInstanceId) {
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seated.in_play([code]))
        .open_turn(INV)
        // Chaos bag content doesn't matter here — no skill test fires.
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .build();
    let instance_id = state.in_play(INV, code);
    (state, INV, instance_id)
}

/// [`state_seated`] with the plain test investigator.
fn state_with_in_play(code: &str) -> (GameState, InvestigatorId, CardInstanceId) {
    state_seated(seat(1), code)
}

fn activate(investigator: InvestigatorId, instance_id: CardInstanceId, index: u8) -> TurnAction {
    TurnAction::ActivateAbility {
        investigator,
        source: AbilitySource::InPlay(instance_id),
        address: AbilityAddress::Printed(index),
    }
}

/// Submit `action`, which the menu does not offer, straight to its handler,
/// and assert the handler refuses it without emitting anything. Returns the
/// state after the refusal.
fn refused_off_menu(session: TestSession, action: &TurnAction) -> GameState {
    let refused = session.submit_off_menu(action);
    refused.expect_rejected();
    let result = refused.finish();
    assert!(result.events.is_empty(), "{:?}", result.events);
    result.state
}

#[test]
fn fast_resource_loop_activates_and_resolves_effect() {
    // Pay 1 resource, gain 1 resource: net zero, but proves the full
    // cost-then-effect ordering and that a `[fast]` ability costs no
    // action.
    let (state, id, instance_id) = state_with_in_play(FAST_RESOURCE_LOOP);
    let actions_before = state.investigators[&id].actions_remaining;

    let result = TestSession::new(state, registries())
        .take(&activate(id, instance_id, 0))
        .finish();
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
    let action = activate(id, instance_id, 0);

    let after_first = TestSession::new(state, registries()).take(&action);
    let inv = &after_first.state().investigators[&id];
    assert_eq!(inv.actions_remaining, actions_before - 1);
    assert_eq!(inv.resources, 5 + 1);
    assert!(
        inv.cards_in_play[0].exhausted,
        "source must exhaust after activation",
    );
    assert_event!(
        after_first.events(),
        Event::CardExhausted { investigator, instance_id: iid, .. }
            if *investigator == id && *iid == instance_id
    );

    // Second activation: source is exhausted; Cost::Exhaust check
    // rejects without mutating state.
    refused_off_menu(after_first, &action);
}

#[test]
fn insufficient_resources_reject_without_payment() {
    let (state, id, instance_id) = state_seated(seat(1).resources(0), FAST_RESOURCE_LOOP);

    let after = refused_off_menu(
        TestSession::new(state, registries()),
        &activate(id, instance_id, 0),
    );
    // Wallet untouched.
    assert_eq!(after.investigators[&id].resources, 0);
}

#[test]
fn insufficient_actions_reject_action_cost_ability() {
    let (state, id, instance_id) = state_seated(seat(1).actions(0), ACTION_EXHAUST_GAIN);

    let after = refused_off_menu(
        TestSession::new(state, registries()),
        &activate(id, instance_id, 0),
    );
    // Source still ready (not exhausted), resources untouched.
    let inv = &after.investigators[&id];
    assert!(!inv.cards_in_play[0].exhausted);
    assert_eq!(inv.resources, 5);
}

#[test]
fn ability_index_pointing_at_non_activated_trigger_rejects() {
    let (state, id, instance_id) = state_with_in_play(CONSTANT_ONLY);

    refused_off_menu(
        TestSession::new(state, registries()),
        &activate(id, instance_id, 0),
    );
}

#[test]
fn ability_index_out_of_bounds_rejects() {
    let (state, id, instance_id) = state_with_in_play(FAST_RESOURCE_LOOP);

    refused_off_menu(
        TestSession::new(state, registries()),
        &activate(id, instance_id, 9),
    );
}

#[test]
fn discard_card_from_hand_cost_with_an_empty_hand_rejects() {
    let (state, id, instance_id) = state_with_in_play(DISCARD_COST_ABILITY);
    assert!(state.investigators[&id].hand.is_empty());

    // No partial mutation: source still ready, no events fired.
    let after = refused_off_menu(
        TestSession::new(state, registries()),
        &activate(id, instance_id, 0),
    );
    assert!(!after.investigators[&id].cards_in_play[0].exhausted);
}

#[test]
fn activating_with_defeated_status_doesnt_need_registry() {
    // Belt-and-suspenders: even with the registry knowing the card, the status
    // check rejects before the registry lookup runs.
    let (mut state, id, instance_id) = state_with_in_play(FAST_RESOURCE_LOOP);
    // No seat setter stages a status.
    state.investigators.get_mut(&id).unwrap().status = Status::Defeated;

    let refused =
        TestSession::new(state, registries()).submit_off_menu(&activate(id, instance_id, 0));
    refused.expect_rejected();
    assert_no_event!(refused.finish().events, Event::AbilityActivated { .. });
}

// ---- test-scoped recorded modifiers ---------------------------------

/// Run an Intellect test at difficulty 4, buying the `SKILL_BOOST` ability on
/// `instance_id` at the test's ST.1 player window, committing nothing and
/// declining the ST.2 (pre-token) window.
fn boosted_intellect_test(session: TestSession, instance_id: CardInstanceId) -> TestSession {
    session
        .test_skill(INV, SkillKind::Intellect, 4)
        .pick(OptionTarget::CardInstance(instance_id))
        // The ST.1 window re-opens after the play resolves; decline it.
        .skip()
        .commit(&[])
        .skip()
}

#[test]
fn a_test_scoped_modifier_buffs_the_test_it_was_bought_during() {
    // Activate SKILL_BOOST at the running test's player window: pay 1
    // resource, record +1 intellect for *this* test. Base 3 intellect + 1
    // exactly clears difficulty 4.
    let (state, id, instance_id) = state_with_in_play(SKILL_BOOST);
    let resources_before = state.investigators[&id].resources;

    let session = boosted_intellect_test(TestSession::new(state, registries()), instance_id);
    session.assert_at_turn_menu(id);
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == id
    );
    assert_eq!(
        session.state().investigators[&id].resources,
        resources_before - 1,
        "the ability's cost was paid",
    );
    assert!(
        session.state().recorded_modifiers.is_empty(),
        "a test-scoped row expires with the test that owns it",
    );
}

#[test]
fn a_test_scoped_modifier_does_not_leak_into_a_second_test() {
    // Buy the boost inside one test, then run a second identical test
    // without buying it: 3 intellect vs difficulty 4 fails by 1.
    let (state, id, instance_id) = state_with_in_play(SKILL_BOOST);

    let second = boosted_intellect_test(TestSession::new(state, registries()), instance_id)
        .test_skill(id, SkillKind::Intellect, 4)
        // Decline both player windows, commit nothing.
        .skip()
        .commit(&[])
        .skip()
        .finish();
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
    let after_activate = TestSession::new(state, registries())
        .test_skill(id, SkillKind::Intellect, 4)
        .pick(OptionTarget::CardInstance(instance_id));

    let in_flight = after_activate
        .state()
        .current_skill_test()
        .expect("the test is still running")
        .id;
    assert_eq!(after_activate.state().recorded_modifiers.len(), 1);
    let row = &after_activate.state().recorded_modifiers[0];
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

/// Regression for #676: a modifier bought "for this skill test" with no test
/// running has no test to attach to, so the activation is refused rather than
/// banked —
/// and a client that submits it over the wire without consulting the menu
/// gets the same answer the menu gave.
#[test]
fn activating_a_test_scoped_modifier_outside_a_test_is_rejected() {
    let (state, id, instance_id) = state_with_in_play(SKILL_BOOST);
    let resources_before = state.investigators[&id].resources;
    let action = activate(id, instance_id, 0);

    assert!(
        !enumerate::legal_actions(&state, registries().cards).contains(&action),
        "the open-turn menu must not offer an activation that would reject",
    );

    let after = refused_off_menu(TestSession::new(state, registries()), &action);
    // State unchanged by the rejection: no resource spent, nothing recorded.
    assert_eq!(after.investigators[&id].resources, resources_before);
    assert!(after.recorded_modifiers.is_empty());
}

/// A "Limit once per round" activated ability is offered once, refused once it
/// has been used, and offered again the next round.
/// `glossary/Limits_and_Maximums.md`: *"Each instance of an ability with such a
/// limit may be initiated X times during the designated period."*
#[test]
fn a_once_per_round_activated_ability_is_offered_once_per_round() {
    let (state, id, instance_id) = state_with_in_play(ONCE_PER_ROUND_GAIN);
    let action = activate(id, instance_id, 0);
    assert!(
        enumerate::legal_actions(&state, registries().cards).contains(&action),
        "an unused limited ability is offered",
    );

    let after_first = TestSession::new(state, registries()).take(&action);
    assert_eq!(after_first.state().investigators[&id].resources, 5 + 1);
    assert!(
        !enumerate::legal_actions(after_first.state(), registries().cards).contains(&action),
        "a once-per-round ability used this round is not offered again",
    );
    let mut next_round = refused_off_menu(after_first, &action);

    next_round.round += 1;
    assert!(
        enumerate::legal_actions(&next_round, registries().cards).contains(&action),
        "the limit resets when the round advances",
    );
}

/// A limited ability on a source the activator reaches without controlling it —
/// here a card attached to their location — records its use on that
/// card, wherever it sits, and is refused for the rest of the round.
#[test]
fn a_limited_ability_on_a_card_the_activator_does_not_control_counts_its_use() {
    // A seat cannot place a location attachment, so its id is named here.
    const ATTACHMENT: CardInstanceId = CardInstanceId(0);
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).at(HERE))
        .with_location(location(HERE).attachments([CardInPlay::enter_play(
            CardCode::new(ONCE_PER_ROUND_GAIN),
            ATTACHMENT,
            Owner::EncounterDeck,
        )]))
        .open_turn(INV)
        .build();
    let action = activate(INV, ATTACHMENT, 0);

    let after_first = TestSession::new(state, registries()).take(&action);
    assert_eq!(after_first.state().investigators[&INV].resources, 5 + 1);
    assert!(
        !enumerate::legal_actions(after_first.state(), registries().cards).contains(&action),
        "the use is counted against the attached card",
    );
}

/// An activated ability whose eligibility condition is false is not offered, and a
/// submission that skips the menu is refused; once the condition holds it is
/// offered: the activation side of the eligibility gate the forced scan
/// applies.
#[test]
fn a_tagged_activated_ability_is_not_offered_while_its_condition_is_false() {
    let (state, id, instance_id) = state_with_in_play(CLUE_GATED_GAIN);
    assert_eq!(state.investigators[&id].clues, 0);
    let action = activate(id, instance_id, 0);
    assert!(
        !enumerate::legal_actions(&state, registries().cards).contains(&action),
        "a false eligibility condition keeps the ability off the menu",
    );
    refused_off_menu(TestSession::new(state.clone(), registries()), &action);

    let mut with_a_clue = state;
    with_a_clue.investigators.get_mut(&id).unwrap().clues = 1;
    assert!(
        enumerate::legal_actions(&with_a_clue, registries().cards).contains(&action),
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
    let state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .at(HERE)
                .in_play([ONCE_PER_ROUND_DISCOVERY])
                .threat_area([CANCEL_DISCOVERY]),
        )
        .with_location(location(HERE).clues(2))
        .open_turn(INV)
        .build();
    let action = activate(INV, state.in_play(INV, ONCE_PER_ROUND_DISCOVERY), 0);
    let interrupt_card = state.card(INV, CANCEL_DISCOVERY);

    let cancelled = TestSession::new(state, registries())
        .take(&action)
        .pick(interrupt_card);

    assert_eq!(
        cancelled.state().investigators[&INV].clues,
        0,
        "the discovery was cancelled",
    );
    assert_eq!(cancelled.state().locations[&HERE].clues, 2);
    assert!(
        !enumerate::legal_actions(cancelled.state(), registries().cards).contains(&action),
        "the cancelled use counts against the once-per-round limit",
    );
}
