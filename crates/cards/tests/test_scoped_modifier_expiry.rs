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
//! **Grasping Hands (01162):** "<b>Revelation</b> - Test [agility] (3). For
//! each point you fail by, take 1 damage."
//! **Guard Dog (01021):** an ally with printed health 3 — used here only as a
//! soaker for that damage.

use card_dsl::dsl::{IntExpr, Stat};
use cards::REGISTRY;
use game_core::assert_event_count;
use game_core::engine::EngineOutcome;
use game_core::event::Event;
use game_core::state::{
    CardCode, ChaosToken, GameState, GameStateBuilder, InvestigatorId, Lifetime, LocationId,
    MythosPhaseFrame, MythosResume, Phase, RecordedModifier, SkillTestId, Status,
};
use game_core::test_support::{self, seat, SeatLookup as _, TestSession};

const GRASPING_HANDS: &str = "01162";
const GUARD_DOG: &str = "01021";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(20);

/// The id the single skill test on either board is minted.
const THE_TEST: SkillTestId = SkillTestId(0);
/// Some other test's id — a row that must survive either teardown.
const ANOTHER_TEST: SkillTestId = SkillTestId(99);

#[ctor::ctor(unsafe)]
fn install_registry() {
    test_support::install_registry_with_test_cards(REGISTRY);
}

/// Mythos step 1.4, the test investigator (8 health) to draw: at a location
/// with `damage` already on them and `soakers` in play, Grasping Hands on top
/// of the encounter deck and a rigged `Numeric(-2)` token. The seeded row for
/// [`THE_TEST`] is in force while it runs: agility 3 + 1 − 2 = 2 vs difficulty
/// 3 → fail by 1 → 1 damage.
fn board(damage: u8, soakers: &[&str]) -> GameState {
    let mut state = GameStateBuilder::new()
        .with_seat(
            seat(1)
                .at(HERE)
                .harm(damage, 0)
                .in_play(soakers.iter().copied()),
        )
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_turn_order([INV])
        .with_phase(Phase::Mythos)
        .with_phase_anchor(MythosPhaseFrame {
            resume: MythosResume::AfterDraws,
        })
        .with_mythos_draw_remaining([INV])
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
        INV,
        Stat::Agility,
        IntExpr::Lit(1),
        Lifetime::SkillTest(test),
        None,
    )
}

/// Draw and reveal the top encounter card for investigator 1, committing
/// nothing to its test.
fn reveal(state: GameState) -> TestSession {
    TestSession::new(state).confirm().commit(&[])
}

/// The rows left after a run, as the tests want to read them.
fn surviving_lifetimes(state: &GameState) -> Vec<Lifetime> {
    state
        .recorded_modifiers
        .iter()
        .map(|m| m.lifetime)
        .collect()
}

#[test]
fn a_row_expires_when_its_test_is_abandoned_on_the_testers_elimination() {
    // The investigator at 7/8 damage takes 1 more → killed. The test is abandoned
    // rather than resolved (#564), and its teardown must still expire the
    // rows bought for it. No soaker, so no distribution prompt.
    let session = reveal(board(7, &[]));
    let state = session.state();

    assert_eq!(state.investigators[&INV].status, Status::Defeated);
    assert_event_count!(session.events(), 1, Event::SkillTestEnded { .. });
    assert_eq!(
        state.skill_test_ids.peek(),
        1,
        "exactly one test ran, so it is SkillTestId(0)",
    );
    assert_eq!(
        surviving_lifetimes(state),
        vec![Lifetime::SkillTest(ANOTHER_TEST)],
        "the abandoned test's row is gone; another test's is untouched",
    );
    // The lone investigator's elimination ends the game: abandoned cleanly.
    assert_eq!(session.finish().outcome, EngineOutcome::Done);
}

#[test]
fn a_row_expires_when_its_test_resolves_across_a_suspension() {
    // Same failed test, but the investigator survives and soaks the point onto
    // Guard Dog — a per-point prompt, so the test's resolution suspends before
    // its teardown runs.
    let state = board(0, &[GUARD_DOG]);
    let dog = state.card(INV, GUARD_DOG);
    let session = reveal(state).pick(dog);

    // Play runs on out of the Mythos phase to the investigator's turn.
    session.assert_at_turn_menu(INV);
    let state = session.state();
    assert_eq!(
        state.investigators[&INV].status,
        Status::Active,
        "1 damage is not lethal at 0/8",
    );
    assert_eq!(
        state.skill_test_ids.peek(),
        1,
        "exactly one test ran, so it is SkillTestId(0)",
    );
    assert_eq!(
        surviving_lifetimes(state),
        vec![Lifetime::SkillTest(ANOTHER_TEST)],
        "the resolved test's row is gone; another test's is untouched",
    );
}
