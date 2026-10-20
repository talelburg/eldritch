//! [`Effect::TreatRevealedToken`]: a constant that makes a chaos token its
//! controller reveals count as another symbol.
//!
//! The Necronomicon 01009 is the corpus consumer (*"Treat each \[elder_sign\]
//! you reveal on a chaos token as a \[auto_fail\]."*,
//! `data/arkhamdb-snapshot/pack/core/core.json`), tested against the real
//! registry in `crates/cards/tests/the_necronomicon.rs`. This binary pins the
//! primitive with two other pairs, so the substitution is not the one pair
//! hard-wired: a `[skull]` counted as the `[elder_sign]` (the counted symbol
//! picks the chaos symbol effect at ST.4 and again at ST.7), and a `[cultist]`
//! counted as an `[auto_fail]` (the counted symbol picks the resolution).
//!
//! Synthetic cards only (ADR 0016). Its own integration-test binary, so its
//! `MockRegistry` gets a process to itself.
//!
//! [`Effect::TreatRevealedToken`]: card_dsl::dsl::Effect::TreatRevealedToken

use std::sync::LazyLock;

use card_dsl::dsl::{self, ChaosSymbol, IntExpr, InvestigatorTarget, TokenOutcome};
use game_core::engine::Registries;
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId, LocationId, RevealedToken,
    SkillKind, TokenModifiers, TokenResolution,
};
use game_core::test_support::{self, seat, MockRegistry, Seat, TestSession, TEST_INV};
use game_core::{assert_event, assert_no_event};

/// Mock asset: *"Treat each \[skull\] you reveal on a chaos token as an
/// \[elder_sign\]."*
const SKULL_AS_ELDER_SIGN: &str = "MOCK-SKULL-ES";
/// Mock asset: *"Treat each \[cultist\] you reveal on a chaos token as an
/// \[auto_fail\]."*
const CULTIST_AS_AUTO_FAIL: &str = "MOCK-CULTIST-AF";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            // The fixture investigator's elder sign: `+2`, and on success gain
            // 2 resources, so both its ST.4 modifier and its ST.7 part show.
            .with_abilities(TEST_INV, || {
                vec![dsl::elder_sign_with(TokenOutcome {
                    on_success: Some(dsl::gain_resources(InvestigatorTarget::You, 2)),
                    ..TokenOutcome::modifier(IntExpr::Lit(2))
                })]
            })
            .with_abilities(SKULL_AS_ELDER_SIGN, || {
                vec![dsl::constant(dsl::treat_revealed_token(
                    ChaosSymbol::Skull,
                    ChaosSymbol::ElderSign,
                ))]
            })
            .with_abilities(CULTIST_AS_AUTO_FAIL, || {
                vec![dsl::constant(dsl::treat_revealed_token(
                    ChaosSymbol::Cultist,
                    ChaosSymbol::AutoFail,
                ))]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// A willpower-3 investigator `id` at `HERE`, with an in-play instance of each
/// code in `in_play`.
fn investigator(id: u32, in_play: &[&str]) -> Seat {
    seat(id)
        .skill(SkillKind::Willpower, 3)
        .at(HERE)
        .in_play(in_play.iter().copied())
}

/// `seats` at `HERE`, drawing only `token` (every scenario symbol `+0`), on
/// `INV`'s open turn.
fn board(token: ChaosToken, seats: impl IntoIterator<Item = Seat>) -> GameState {
    let mut builder = GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_chaos_bag(ChaosBag::new([token]))
        .with_token_modifiers(TokenModifiers::default());
    for seat in seats {
        builder = builder.with_seat(seat);
    }
    builder.open_turn(INV).build()
}

/// `INV`'s willpower test at `difficulty` on `state`, committing nothing, run
/// to the end.
fn willpower_test(state: GameState, difficulty: i8) -> TestSession {
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(&[]);
    session.assert_at_turn_menu(INV);
    session
}

/// Willpower 3 against 5: a `[skull]` at `+0` fails it, the elder sign's `+2`
/// passes it. Counted as the elder sign, the skull resolves as the elder sign
/// at ST.4 (its `+2`) and at ST.7 (its success part).
#[test]
fn a_skull_counted_as_the_elder_sign_resolves_as_the_elder_sign() {
    let session = willpower_test(
        board(ChaosToken::Skull, [investigator(1, &[SKULL_AS_ELDER_SIGN])]),
        5,
    );
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::ElderSign,
        }
    );
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 0, .. }
    );
    assert_event!(
        session.events(),
        Event::ResourcesGained {
            investigator: INV,
            amount: 2,
        }
    );
}

/// The control: with nothing substituting, the same skull is a `+0` skull, and
/// the test fails with no elder-sign success part.
#[test]
fn without_the_constant_a_skull_resolves_as_a_skull() {
    let session = willpower_test(board(ChaosToken::Skull, [investigator(1, &[])]), 5);
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::Modifier(0),
        }
    );
    assert_event!(session.events(), Event::SkillTestFailed { by: 2, .. });
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
}

/// Willpower 3 against 0 passes on the numbers; a `[cultist]` counted as an
/// `[auto_fail]` fails it anyway.
#[test]
fn a_cultist_counted_as_an_auto_fail_fails_a_test_the_numbers_pass() {
    let session = willpower_test(
        board(
            ChaosToken::Cultist,
            [investigator(1, &[CULTIST_AS_AUTO_FAIL])],
        ),
        0,
    );
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Cultist,
            resolution: TokenResolution::AutoFail,
        }
    );
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::SkillTestSucceeded { .. });
}

/// *"you reveal"*: the constant reads off the cards of the investigator
/// performing the test. Another investigator's copy substitutes nothing.
#[test]
fn another_investigators_constant_does_not_substitute() {
    let session = willpower_test(
        board(
            ChaosToken::Skull,
            [
                investigator(1, &[]),
                investigator(2, &[SKULL_AS_ELDER_SIGN]),
            ],
        ),
        5,
    );
    assert_event!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            resolution: TokenResolution::Modifier(0),
        }
    );
    assert_event!(session.events(), Event::SkillTestFailed { .. });
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
}

/// The frame keeps both: the token that came out of the bag and the symbol it
/// counts as. Read at the post-ST.6 acknowledge, where the frame is still open.
#[test]
fn the_frame_records_the_drawn_token_and_what_it_counts_as() {
    let mut state = board(ChaosToken::Skull, [investigator(1, &[SKULL_AS_ELDER_SIGN])]);
    state.interactive_acknowledge = true;
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, 5)
        .commit(&[]);
    let revealed = session
        .state()
        .current_skill_test()
        .expect("the acknowledge pause holds the test frame open")
        .revealed;
    assert_eq!(
        revealed,
        Some(RevealedToken {
            token: ChaosToken::Skull,
            counts_as: ChaosToken::ElderSign,
            resolution: TokenResolution::ElderSign,
        })
    );
}
