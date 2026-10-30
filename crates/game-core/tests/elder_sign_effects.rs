//! An investigator's `[elder_sign]` as a whole [`TokenOutcome`] end to end:
//! its success and failure parts resolve at ST.7, and its unconditional part
//! at ST.4.
//!
//! ST.4 (`data/rules-reference/rules/Appendix_II_Timing_and_Gameplay.md`):
//!
//! > The [elder_sign] symbol indicates that the [elder_sign] ability on the
//! > investigator card belonging to the player performing the test must
//! > initiate.
//!
//! ST.7, same file:
//!
//! > The card ability or game rule that initiated a skill test usually
//! > indicates the consequences of success and/or failure for that test.
//! > (Additionally, some other card abilities may contribute additional
//! > consequences, or modify existing consequences, at this time.) Resolve the
//! > appropriate consequences (based on the success or failure established
//! > during step ST.6) at this time.
//!
//! Synthetic investigators only (ADR 0016): each models one elder-sign shape
//! the primitive has to carry, and none stands in for a printed card. Its own
//! integration-test binary, so its `MockRegistry` gets a process to itself.
//! Every assertion is on events or state crossing the `apply` boundary.

use std::sync::LazyLock;

use card_dsl::dsl::{
    self, Determination, Effect, HarmKind, IntExpr, InvestigatorTarget, TokenOutcome,
};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId, LocationId,
    RecordedModifierKind, SkillKind,
};
use game_core::test_support::{location, seat, test_investigator_card, MockRegistry, TestSession};
use game_core::{assert_event, assert_no_event};

/// Elder sign: modifier `+2`, and an `on_success` arm that gains 2 resources.
const ON_SUCCESS: &str = "_es_on_success";
/// Elder sign: modifier `+0`, and an `on_fail` arm that deals 1 horror.
const ON_FAIL: &str = "_es_on_fail";
/// Elder sign: modifier `+0`, and an `immediate` arm that is
/// `AutoResolve(AutomaticSuccess)`, unconditionally. Wendy Adams 01005 is
/// the motivating consumer, and registers with her own condition.
const AUTO_SUCCESS: &str = "_es_auto_success";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_card(test_investigator_card(ON_SUCCESS))
            .with_abilities(ON_SUCCESS, || {
                vec![dsl::elder_sign_with(TokenOutcome {
                    on_success: Some(dsl::gain_resources(InvestigatorTarget::You, 2)),
                    ..TokenOutcome::modifier(IntExpr::Lit(2))
                })]
            })
            .with_card(test_investigator_card(ON_FAIL))
            .with_abilities(ON_FAIL, || {
                vec![dsl::elder_sign_with(TokenOutcome {
                    on_fail: Some(Effect::Deal {
                        kind: HarmKind::Horror,
                        target: InvestigatorTarget::You,
                        amount: IntExpr::Lit(1),
                        direct: false,
                    }),
                    ..TokenOutcome::default()
                })]
            })
            .with_card(test_investigator_card(AUTO_SUCCESS))
            .with_abilities(AUTO_SUCCESS, || {
                vec![dsl::elder_sign_with(TokenOutcome {
                    immediate: Some(dsl::auto_resolve(Determination::AutomaticSuccess)),
                    ..TokenOutcome::default()
                })]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// The investigator printed as `code`, at `HERE` (holding `clues` clues),
/// drawing only `[elder_sign]`, with play at their turn menu.
fn session(code: &str, clues: u8) -> TestSession {
    TestSession::new(board(code, clues), registries())
}

/// The board [`session`] starts from.
fn board(code: &str, clues: u8) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(seat(1).as_card(code).at(HERE))
        .with_location(location(HERE).clues(clues))
        .with_chaos_bag(ChaosBag::new([ChaosToken::ElderSign]))
        .open_turn(INV)
        .build()
}

/// A plain willpower test at `difficulty`, committing nothing, run to the end.
fn willpower_test(code: &str, difficulty: i8) -> TestSession {
    let session = session(code, 0)
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

#[test]
fn an_elder_signs_success_part_resolves_only_on_success() {
    // Willpower 3 + 2 against 5 passes; against 6 it fails.
    let pass = willpower_test(ON_SUCCESS, 5);
    assert_event!(
        pass.events(),
        Event::ResourcesGained {
            investigator: INV,
            amount: 2,
        }
    );
    let fail = willpower_test(ON_SUCCESS, 6);
    assert_event!(fail.events(), Event::SkillTestFailed { .. });
    assert_no_event!(fail.events(), Event::ResourcesGained { .. });
}

#[test]
fn an_elder_signs_success_part_follows_the_clue_discovery() {
    // Intellect 3 + 2 against shroud 2 passes. ST.7 resolves the action's
    // own consequence (the clue) before the elder sign's success part.
    let session = session(ON_SUCCESS, 1)
        .take(&TurnAction::Investigate { investigator: INV })
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    let events = session.events();
    let clue = events
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::CluePlaced {
                    investigator: INV,
                    count: 1
                }
            )
        })
        .expect("the investigation discovers the clue");
    let resources = events
        .iter()
        .position(|e| {
            matches!(
                e,
                Event::ResourcesGained {
                    investigator: INV,
                    amount: 2,
                }
            )
        })
        .expect("the elder sign's success part gains 2 resources");
    assert!(
        clue < resources,
        "the success part resolves after the clue discovery",
    );
}

#[test]
fn an_elder_signs_failure_part_resolves_only_on_failure() {
    // Willpower 3 + 0 against 3 passes; against 4 it fails.
    let pass = willpower_test(ON_FAIL, 3);
    assert_event!(pass.events(), Event::SkillTestSucceeded { .. });
    assert_no_event!(pass.events(), Event::HorrorPlacedOnInvestigator { .. });
    let fail = willpower_test(ON_FAIL, 4);
    assert_event!(
        fail.events(),
        Event::HorrorPlacedOnInvestigator {
            investigator: INV,
            amount: 1,
        }
    );
    assert_eq!(fail.state().investigators[&INV].horror(), 1);
}

/// `glossary/Automatic_Failure_Success.md`: *"If a chaos token effect causes
/// an investigator to automatically succeed or fail at a skill test, continue
/// with Steps 3 and 4, as normal."* Wendy Adams's ruling
/// (<https://arkhamdb.com/card/01005>): *"If a skill test automatically
/// succeeds, the total difficulty of that test is considered 0."*
#[test]
fn an_elder_signs_immediate_automatic_success_wins_a_test_the_numbers_lose() {
    // Willpower 3 + 0 against 6 fails on the numbers.
    let session = willpower_test(AUTO_SUCCESS, 6);
    let card = session.state().investigators[&INV]
        .investigator_card
        .instance_id;
    // ST.3 still reveals the token.
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::ElderSign,
            ..
        }
    );
    // The elder sign's ST.4 part latches the determination, named after the
    // investigator card it came from.
    assert_event!(
        session.events(),
        Event::SkillTestDeterminationLatched {
            investigator: INV,
            determination: Determination::AutomaticSuccess,
            source: Some(source),
        } if *source == card
    );
    // ST.5 still determines the modified skill value (3), and the difficulty
    // is 0, so the test succeeds by 3.
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded {
            investigator: INV,
            margin: 3,
            ..
        }
    );
    // The latched determination's recorded row names the investigator card
    // too. Read at the post-ST.6 acknowledge, before ST.8 sweeps the row.
    let mut state = board(AUTO_SUCCESS, 0);
    state.interactive_acknowledge = true;
    let paused = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, 6)
        .commit(&[]);
    let state = paused.state();
    let card = state.investigators[&INV].investigator_card.instance_id;
    let row = state
        .recorded_modifiers
        .iter()
        .find(|row| {
            matches!(
                row.kind,
                RecordedModifierKind::Determination(Determination::AutomaticSuccess)
            )
        })
        .expect("the elder sign latches an automatic-success row");
    assert_eq!(row.source, Some(card));
}
