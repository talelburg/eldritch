//! §G per-cell eligibility re-scan (EmitEvent-frame C-coordinators, #434).
//!
//! The `RoundEnded` coordinator walks `when → at → after`, re-scanning each cell
//! fresh — a `when` reaction can change whether an `at` forced even fires, so the
//! grid is **not** pre-computed. No in-scope corpus card exercises cross-bucket
//! suppression at one round-end emit, so this uses a synthetic mock registry:
//!
//! - a test **act** carries a `When`-`RoundEnded` reaction whose native removes a
//!   threat-area card from the game;
//! - that threat-area card (`TESTX`) carries an `At`-`RoundEnded` *forced* ability
//!   that gives its controller a resource.
//!
//! Picking the `when` reaction removes `TESTX` before the `at` cell is scanned, so
//! the `at` forced does **not** fire (no resource gained). Skipping leaves `TESTX`
//! in play, so the `at` forced fires (one resource). The difference is the re-scan.

use std::sync::LazyLock;

use card_dsl::dsl::{self, EventPattern, EventTiming, InvestigatorTarget};
use game_core::engine::evaluator::EvalContext;
use game_core::engine::{EngineOutcome, NativeCx, OptionTarget, Registries};
use game_core::scenario::ScenarioRegistry;
use game_core::state::{Act, CardCode, CardInstanceId, GameStateBuilder, InvestigatorId};
use game_core::test_support::{seat, MockRegistry, TestSession};

const TEST_ACT: &str = "TESTACT";
const TEST_X: &str = "TESTX";

/// `when`-cell native: remove every `TESTX` in an investigator's threat area
/// from the game — flipping the `at`-cell forced's eligibility (its source
/// leaves play).
fn when_remove_x(cx: &mut NativeCx, _ctx: &EvalContext) -> EngineOutcome {
    let xs: Vec<CardInstanceId> = cx
        .state()
        .investigators
        .values()
        .flat_map(|inv| &inv.threat_area)
        .filter(|c| c.code.as_str() == TEST_X)
        .map(|c| c.instance_id)
        .collect();
    for x in xs {
        cx.remove_from_game(x);
    }
    EngineOutcome::Done
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(TEST_ACT, || {
                vec![dsl::reaction_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::When,
                    dsl::native("when:remove_x"),
                )]
            })
            .with_abilities(TEST_X, || {
                // Give the controller (the card's owner) one resource: the
                // observable signal that the `at` forced fired.
                vec![dsl::forced_on_event(
                    EventPattern::RoundEnded,
                    EventTiming::At,
                    dsl::gain_resources(InvestigatorTarget::You, 1),
                )]
            })
            .with_native_effect("when:remove_x", when_remove_x)
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The lead's resources.
fn resources(session: &TestSession) -> u8 {
    session.state().investigators[&InvestigatorId(1)].resources
}

/// The end of the Upkeep phase, the test act current, the lead holding `TESTX`
/// (the `at`-forced source) in their threat area. Settled, it rests
/// at the round end's `when` reaction window.
fn round_end_window() -> TestSession {
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .ending_upkeep_phase()
        // `TESTX` is a code the registry doesn't know, so the encounter deck
        // owns it.
        .with_seat(seat(1).threat_area([TEST_X]))
        .with_turn_order([InvestigatorId(1)])
        .build();
    state.act_deck = vec![Act {
        code: CardCode::new(TEST_ACT),
        clue_threshold: 0,
    }];
    state.act_index = 0;
    TestSession::new(state, registries())
}

#[test]
fn when_cell_picked_suppresses_the_at_forced() {
    // Pick the `when` reaction (sole candidate): it removes TESTX before the
    // `at` cell is scanned, so the `at` forced never fires.
    let window = round_end_window();
    let before = resources(&window);
    let session = window.pick(OptionTarget::Act);
    assert_eq!(
        resources(&session) - before,
        0,
        "the `at` forced must NOT fire: the `when` cell removed its source (per-cell re-scan)"
    );
}

#[test]
fn when_cell_skipped_leaves_the_at_forced_eligible() {
    // Control: skip the `when` reaction → TESTX stays in play → the `at` forced
    // fires (one resource). Isolates "the re-scan suppressed it" from "it never fired".
    let window = round_end_window();
    let before = resources(&window);
    let session = window.skip();
    assert_eq!(
        resources(&session) - before,
        1,
        "the `at` forced fires when the `when` cell leaves its source in play"
    );
}
