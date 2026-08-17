//! A test-scoped modifier expires with the test it names — including when
//! that test ends unusually (#676).
//!
//! The happy path is covered by `hyperawareness.rs`, which buys a real
//! `ThisSkillTest` buff at a running test's player window and finds it gone
//! afterwards. This file covers the two endings where a leak would be
//! invisible: a test **abandoned** because its tester was eliminated
//! mid-resolution (#564), and one that **resolves across a suspension** (the
//! per-point damage-distribution prompts, #422). Both go through a teardown
//! that is easy to miss, and a surviving row would silently buff whatever
//! test came next.
//!
//! Rows are seeded directly rather than bought from a card: the buff has to
//! be *in flight* when the unusual ending happens, and neither board reaches
//! a window where Hyperawareness could be bought. Both boards run exactly
//! one skill test, so it is minted `SkillTestId(0)` — asserted, not assumed.
//!
//! ## Verified card text (`data/arkhamdb-snapshot`, 2026-07-17)
//!
//! **Grasping Hands (01162):** "<b>Revelation</b> - Test [agility] (3). If you
//! fail, take 1 damage for each point you failed by."
//! **Guard Dog (01021):** an ally with health 3, used here only as a soaker.

use game_core::action::EngineRecord;
use game_core::engine::OptionId;
use game_core::event::Event;
use game_core::state::{
    CardCode, CardInPlay, CardInstanceId, ChaosToken, InvestigatorId, Lifetime, LocationId,
    RecordedModifier, SkillTestId, Status,
};
use game_core::test_support::{
    drive, test_investigator, test_location, GameStateBuilder, ScriptedResolver,
};
use game_core::{assert_event_count, Action, EngineOutcome};

const GRASPING_HANDS: &str = "01162";
const GUARD_DOG: &str = "01021";
/// Roland Banks — health 9, sanity 5.
const ROLAND: &str = "01001";

/// The id the single skill test on either board is minted.
const THE_TEST: SkillTestId = SkillTestId(0);
/// Some other test's id — a row that must survive either teardown.
const ANOTHER_TEST: SkillTestId = SkillTestId(99);

#[ctor::ctor(unsafe)]
fn install_registry() {
    let _ = game_core::card_registry::install(cards::REGISTRY);
}

/// Roland at a location with `damage` already on him and `soakers` in play,
/// Grasping Hands on top of the encounter deck and a rigged `Numeric(-2)`
/// token: agility 3 − 2 = 1 vs difficulty 3 → fail by 2 → 2 damage.
fn board(damage: u8, soakers: &[&str]) -> game_core::GameState {
    let mut inv = test_investigator(1);
    // Real investigator code so max_health() reads capacity from the
    // installed corpus registry (#448).
    inv.investigator_card.code = CardCode::new(ROLAND);
    inv.investigator_card.accumulated_damage = damage;
    inv.cards_in_play = soakers
        .iter()
        .enumerate()
        .map(|(i, code)| {
            CardInPlay::enter_play(
                CardCode::new(*code),
                CardInstanceId(u32::try_from(i).expect("fits") + 1),
            )
        })
        .collect();
    let mut state = GameStateBuilder::new()
        .with_investigator_at(inv, LocationId(20))
        .with_location(test_location(20, "Here"))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.chaos_bag.tokens = vec![ChaosToken::Numeric(-2)];
    state
        .encounter_deck
        .push_back(CardCode::new(GRASPING_HANDS));
    // One row for the test this board is about to run, one for a different
    // test entirely. The first must go; the second must stay.
    state.recorded_modifiers = vec![row_for(THE_TEST), row_for(ANOTHER_TEST)];
    state
}

fn row_for(test: SkillTestId) -> RecordedModifier {
    RecordedModifier::new(
        InvestigatorId(1),
        game_core::dsl::Stat::Agility,
        game_core::dsl::IntExpr::Lit(1),
        Lifetime::SkillTest(test),
        None,
    )
}

/// Reveal the top encounter card for investigator 1, committing nothing and
/// answering each per-point damage-distribution prompt with `picks`.
fn reveal(state: game_core::GameState, picks: &[u32]) -> game_core::ApplyResult {
    let mut resolver = ScriptedResolver::new();
    resolver.commit_cards(&[]);
    for &p in picks {
        resolver.pick_single(OptionId(p));
    }
    drive(
        state,
        Action::Engine(EngineRecord::EncounterCardRevealed {
            investigator: InvestigatorId(1),
        }),
        resolver,
    )
}

/// The rows left after a run, as the tests want to read them.
fn surviving_lifetimes(result: &game_core::ApplyResult) -> Vec<Lifetime> {
    result
        .state
        .recorded_modifiers
        .iter()
        .map(|m| m.lifetime)
        .collect()
}

#[test]
fn a_row_expires_when_its_test_is_abandoned_on_the_testers_elimination() {
    // Roland at 8/9 damage takes 2 more → killed. The test is abandoned
    // rather than resolved (#564), and its teardown must still expire the
    // rows bought for it. No soaker, so no distribution prompt.
    let result = reveal(board(8, &[]), &[]);

    assert_eq!(result.outcome, EngineOutcome::Done, "abandoned cleanly");
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].status,
        Status::Killed,
    );
    assert_event_count!(result.events, 1, Event::SkillTestEnded { .. });
    assert_eq!(
        result.state.skill_test_ids.peek(),
        1,
        "exactly one test ran, so it is SkillTestId(0)",
    );
    assert_eq!(
        surviving_lifetimes(&result),
        vec![Lifetime::SkillTest(ANOTHER_TEST)],
        "the abandoned test's row is gone; another test's is untouched",
    );
}

#[test]
fn a_row_expires_when_its_test_resolves_across_a_suspension() {
    // Same failed test, but Roland survives and soaks both points onto Guard
    // Dog — two per-point prompts, so the test's resolution suspends twice
    // before its teardown runs.
    let result = reveal(board(0, &[GUARD_DOG]), &[1, 1]);

    assert_eq!(result.outcome, EngineOutcome::Done);
    assert_eq!(
        result.state.investigators[&InvestigatorId(1)].status,
        Status::Active,
        "2 damage is not lethal at 0/9",
    );
    assert_eq!(
        result.state.skill_test_ids.peek(),
        1,
        "exactly one test ran, so it is SkillTestId(0)",
    );
    assert_eq!(
        surviving_lifetimes(&result),
        vec![Lifetime::SkillTest(ANOTHER_TEST)],
        "the resolved test's row is gone; another test's is untouched",
    );
}
