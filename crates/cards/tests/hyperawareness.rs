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

use cards::TEST_REGISTRIES;
use game_core::engine::enumerate::{self, TurnAction};
use game_core::engine::OptionTarget;
use game_core::event::Event;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, GameState, GameStateBuilder,
    InvestigatorId, SkillKind, TokenModifiers,
};
use game_core::test_support::{seat, SeatLookup as _, TestSession};
use game_core::{assert_event, assert_no_event};

const HYPERAWARENESS: &str = "01034";
const INV: InvestigatorId = InvestigatorId(1);

/// The printed order of Hyperawareness's two `[fast]` abilities: both are
/// offered on the card, so they are told apart by it.
const INTELLECT_ABILITY: usize = 0;
const AGILITY_ABILITY: usize = 1;

/// A session with one Hyperawareness already in play, the controller at
/// their open turn with `resources` resources, and a single `Numeric(0)`
/// chaos bag for predictable arithmetic. We seed in play (rather than playing
/// from hand) because the activation flow is what this file tests; `PlayCard`
/// is exercised elsewhere.
fn with_hyperawareness(resources: u8) -> TestSession {
    GameStateBuilder::new()
        .with_registries(TEST_REGISTRIES)
        .with_seat(seat(1).resources(resources).in_play([HYPERAWARENESS]))
        .open_turn(INV)
        .with_chaos_bag(ChaosBag::new([ChaosToken::Numeric(0)]))
        .with_token_modifiers(TokenModifiers::default())
        .session()
}

/// The anchor of the seated Hyperawareness: both its abilities are offered on
/// it.
fn hyperawareness(state: &GameState) -> OptionTarget {
    state.card(INV, HYPERAWARENESS)
}

/// Run a `skill` test at `difficulty` from the turn menu, buying `ability`
/// at the test's ST.1 player window and nothing more: pass the window once
/// bought, commit nothing, and pass the window after the commit. The test
/// comes back to the turn menu.
fn test_buying(
    session: TestSession,
    skill: SkillKind,
    difficulty: i8,
    ability: usize,
) -> TestSession {
    let card = hyperawareness(session.state());
    let session = session
        .test_skill(INV, skill, difficulty)
        .pick_nth_at(card, ability)
        .skip()
        .commit(&[])
        .skip();
    session.assert_at_turn_menu(INV);
    session
}

/// [`test_buying`] with nothing bought: both windows passed.
fn test_buying_nothing(session: TestSession, skill: SkillKind, difficulty: i8) -> TestSession {
    let session = session
        .test_skill(INV, skill, difficulty)
        .skip()
        .commit(&[])
        .skip();
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn intellect_ability_buffs_the_intellect_test_it_is_bought_during() {
    // 3 intellect + 1 (the recorded row) + 0 (token) = 4 vs difficulty 4 →
    // succeed by 0. The base 3-intellect investigator would fail without it.
    let session = with_hyperawareness(5);
    let resources_before = session.state().investigators[&INV].resources;

    let result = test_buying(session, SkillKind::Intellect, 4, INTELLECT_ABILITY);

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Intellect, margin: 0 }
            if *investigator == INV
    );
    assert_eq!(
        result.state().investigators[&INV].resources,
        resources_before - 1,
        "1 resource paid",
    );
    assert!(
        result.state().recorded_modifiers.is_empty(),
        "a test-scoped row expires with the test it names",
    );
}

#[test]
fn agility_ability_buffs_the_agility_test_it_is_bought_during() {
    // Same as above but the second ability buffs agility.
    let result = test_buying(
        with_hyperawareness(5),
        SkillKind::Agility,
        4,
        AGILITY_ABILITY,
    );

    assert_event!(
        result.events(),
        Event::SkillTestSucceeded { investigator, skill: SkillKind::Agility, margin: 0 }
            if *investigator == INV
    );
}

#[test]
fn intellect_ability_does_not_buff_an_agility_test() {
    // The recorded row targets `Stat::Intellect`; an agility test ignores
    // it. 3 + 0 < 4 → fail by 1.
    let result = test_buying(
        with_hyperawareness(5),
        SkillKind::Agility,
        4,
        INTELLECT_ABILITY,
    );

    assert_event!(
        result.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Agility, by: 1, .. }
            if *investigator == INV
    );
}

/// The buff belongs to the test it was bought during, and to no other: the
/// next test is unbuffed (#676).
#[test]
fn the_buff_does_not_survive_into_the_next_test() {
    let first = test_buying(
        with_hyperawareness(5),
        SkillKind::Intellect,
        4,
        INTELLECT_ABILITY,
    );
    assert_event!(
        first.events(),
        Event::SkillTestSucceeded {
            skill: SkillKind::Intellect,
            ..
        }
    );
    let seen = first.events().len();

    let second = test_buying_nothing(first, SkillKind::Intellect, 4);
    assert_event!(
        &second.events()[seen..],
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == INV
    );
}

/// Bought at an open-turn menu with no test running, the modifier has no
/// test to attach to — so the activation is **refused**, and the test that
/// follows shows no bonus. Before #676 the resource was spent, nothing
/// visible happened, and the `+1` surfaced on whatever test came next.
#[test]
fn activation_with_no_test_in_flight_is_rejected_and_buffs_nothing_later() {
    let session = with_hyperawareness(5);
    let resources_before = session.state().investigators[&INV].resources;
    let action = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, HYPERAWARENESS)),
        address: AbilityAddress::Printed(0),
    };

    assert!(
        !enumerate::legal_actions(session.state(), TEST_REGISTRIES.cards).contains(&action),
        "the open-turn menu must not offer a buff that cannot be bought",
    );

    // The refusal comes from the RR initiation gate, which proves the effect
    // inert with no test to attach to — the same predicate that kept it off
    // the menu above. (The evaluator's own "no skill test in flight" rejection
    // sits behind it, for the non-activation paths; it is unit-tested in
    // `engine::evaluator`.) The menu does not offer it, so it goes straight to
    // the handler.
    let rejected = session.submit_off_menu(&action);
    let reason = rejected.expect_rejected();
    assert!(
        reason.contains(HYPERAWARENESS) && reason.contains("cannot be initiated"),
        "the reason must name the card and the problem; got: {reason}",
    );
    assert_no_event!(rejected.events(), Event::AbilityActivated { .. });
    assert_eq!(
        rejected.state().investigators[&INV].resources,
        resources_before,
        "a refused activation costs nothing",
    );
    assert!(rejected.state().recorded_modifiers.is_empty());

    // And the test that follows is unbuffed: 3 intellect vs difficulty 4.
    let after_test = test_buying_nothing(rejected, SkillKind::Intellect, 4);
    assert_event!(
        after_test.events(),
        Event::SkillTestFailed { investigator, skill: SkillKind::Intellect, by: 1, .. }
            if *investigator == INV
    );
}

#[test]
fn activation_rejects_when_controller_lacks_a_resource() {
    // Inside a test (where the ability is buyable at all), with an empty
    // wallet: the cost gate refuses and nothing is recorded. Nothing is
    // offered, so the ST.1 window passes on its own and the test rests at its
    // commit window, where the activation is submitted straight to the
    // handler.
    let session = with_hyperawareness(0);
    let action = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(session.state().in_play(INV, HYPERAWARENESS)),
        address: AbilityAddress::Printed(0),
    };
    let started = session.test_skill(INV, SkillKind::Intellect, 4);
    let seen = started.events().len();

    let result = started.submit_off_menu(&action);
    let _ = result.expect_rejected();
    assert_eq!(result.events().len(), seen, "a rejection emits no events");
    // No partial mutation: resources still 0, nothing recorded.
    assert_eq!(result.state().investigators[&INV].resources, 0);
    assert!(result.state().recorded_modifiers.is_empty());
}

#[test]
fn buying_the_buff_costs_no_action_points() {
    // `[fast]` activation must not spend an action.
    let session = with_hyperawareness(5);
    let actions_before = session.state().investigators[&INV].actions_remaining;

    let result = test_buying(session, SkillKind::Intellect, 4, INTELLECT_ABILITY);

    assert_eq!(
        result.state().investigators[&INV].actions_remaining,
        actions_before,
        "a fast ability spends no action",
    );
    assert_event!(result.events(), Event::AbilityActivated { .. });
}
