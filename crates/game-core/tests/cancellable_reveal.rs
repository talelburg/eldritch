//! A chaos-token reveal that can be cancelled: Skill Test Timing step 3 draws
//! and announces the token, then emits the coordinator-owned
//! `ChaosTokenRevealed` condition, whose resolve step is what the token does to
//! the test.
//!
//! A `when`-cell cancel skips it; [`Effect::RevealChaosToken`] sends the test
//! back to draw again.
//!
//! Wendy Adams 01005 is the corpus consumer (*"\[reaction\] When you reveal a
//! chaos token, choose and discard 1 card from your hand: Cancel that chaos
//! token and return it to the bag. Reveal a new chaos token. (Limit once per
//! test/ability.)"*, `data/arkhamdb-snapshot/pack/core/core.json`), tested
//! against the real registry in `crates/cards/tests/wendy_adams.rs`. Her
//! ruling is what the split builds
//! by construction (<https://arkhamdb.com/card/01005>): *"treat it as if
//! you've never revealed that token, for purposes of effects that trigger if a
//! specific symbol is revealed"*. This binary pins the engine half with a
//! mock "cancel and redraw" reaction, minus Wendy's discard cost.
//!
//! Synthetic cards only (ADR 0016). Its own integration-test binary, so its
//! `MockRegistry` gets a process to itself.
//!
//! [`Effect::RevealChaosToken`]: card_dsl::dsl::Effect::RevealChaosToken

use std::sync::LazyLock;

use card_dsl::dsl::{
    self, ChaosSymbol, Cost, Determination, Effect, EventPattern, EventTiming, IntExpr,
    InvestigatorTarget, RevealClause, TokenOutcome, UsageLimit, UsagePeriod,
};
use game_core::engine::enumerate::TurnAction;
use game_core::engine::{OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::ScenarioRegistry;
use game_core::state::{
    AbilityAddress, AbilitySource, ChaosBag, ChaosToken, Enemy, EnemyId, GameState,
    GameStateBuilder, InvestigatorId, LocationId, SkillKind, TokenResolution,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession, TEST_INV};
use game_core::{assert_event, assert_event_count, assert_event_sequence, assert_no_event};

/// Mock asset: *"\[reaction\] When you reveal a chaos token: Cancel that chaos
/// token and return it to the bag. Reveal a new chaos token. (Limit once per
/// test.)"* — Wendy Adams 01005's reaction without its discard cost.
const CANCEL_AND_REDRAW: &str = "MOCK-CANCEL-REDRAW";

/// Mock asset: *"\[reaction\] When you reveal a chaos token: Cancel that chaos
/// token."* — a malformed card, cancelling with nothing revealing a new token.
const CANCEL_ONLY: &str = "MOCK-CANCEL-ONLY";

/// Mock asset: *"\[fast\]: This skill test automatically succeeds."* — a
/// determination latched before step 3.
const AUTO_SUCCEED: &str = "MOCK-AUTO-SUCCEED";

/// Mock asset: *"\[action\]: Fight. If a \[skull\] symbol is revealed during
/// this attack, gain 3 resources."* — a reveal clause with an observable
/// effect.
const SKULL_CLAUSE_FIGHT: &str = "MOCK-SKULL-FIGHT";

const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);
const GHOUL: EnemyId = EnemyId(100);

/// The fixture investigator's elder sign: `+5`, and immediately gain 1
/// resource — a modifier and a chaos symbol effect, both observable.
const ELDER_SIGN_MODIFIER: i8 = 5;

/// A seed whose first two draws from a two-token bag take the first token,
/// then the second: so `[A, B]` reveals `A`, then `B` on the redraw.
const FIRST_THEN_SECOND: u64 = 0;

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            .with_abilities(TEST_INV, || {
                vec![dsl::elder_sign_with(TokenOutcome {
                    immediate: Some(dsl::gain_resources(InvestigatorTarget::You, 1)),
                    ..TokenOutcome::modifier(IntExpr::Lit(ELDER_SIGN_MODIFIER))
                })]
            })
            .with_abilities(CANCEL_AND_REDRAW, || {
                vec![dsl::reaction_on_event(
                    EventPattern::ChaosTokenRevealed,
                    EventTiming::When,
                    dsl::seq([Effect::Cancel, Effect::RevealChaosToken]),
                )
                .with_usage_limit(UsageLimit {
                    count: 1,
                    period: UsagePeriod::SkillTest,
                })]
            })
            .with_abilities(CANCEL_ONLY, || {
                vec![dsl::reaction_on_event(
                    EventPattern::ChaosTokenRevealed,
                    EventTiming::When,
                    Effect::Cancel,
                )]
            })
            .with_abilities(AUTO_SUCCEED, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::Resources(1)],
                    dsl::auto_resolve(Determination::AutomaticSuccess),
                )]
            })
            .with_abilities(SKULL_CLAUSE_FIGHT, || {
                vec![dsl::activated_as(
                    dsl::fight(0i8, 0u8).with_reveal_clause(RevealClause {
                        symbols: vec![ChaosSymbol::Skull],
                        effect: Box::new(dsl::gain_resources(InvestigatorTarget::You, 3)),
                    }),
                    1,
                    vec![],
                    dsl::seq([]),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry::EMPTY,
    });
    *REGISTRIES
}

/// A willpower-3 investigator at `HERE` with an in-play instance of each code
/// in `in_play`, drawing from `bag` under `seed`, on their open turn.
fn board(in_play: &[&str], bag: impl IntoIterator<Item = ChaosToken>, seed: u64) -> GameState {
    GameStateBuilder::new()
        .with_registries(registries())
        .with_location(test_support::test_location(HERE.0, "Here"))
        .with_seat(
            seat(INV.0)
                .skill(SkillKind::Willpower, 3)
                .resources(5)
                .at(HERE)
                .in_play(in_play.iter().copied()),
        )
        .with_chaos_bag(ChaosBag::new(bag))
        .with_rng_seed(seed)
        .open_turn(INV)
        .build()
}

/// `INV`'s willpower test at `difficulty`, committing nothing: the session
/// rests at the first reveal's `when` cell when a card listens there.
fn willpower_test(state: GameState, difficulty: i8) -> TestSession {
    TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, difficulty)
        .commit(&[])
}

/// The cancel-and-redraw reaction, offered on its in-play card.
fn cancel_and_redraw(session: &TestSession) -> OptionTarget {
    session.state().card(INV, CANCEL_AND_REDRAW)
}

/// Use the cancel-and-redraw reaction at the open `when` cell.
fn pick_cancel_and_redraw(session: TestSession) -> TestSession {
    let reaction = cancel_and_redraw(&session);
    session.pick(reaction)
}

/// Willpower 3 against 5. The elder sign's `+5` passes it and gains a
/// resource at ST.4; cancelled, it does neither, and the `0` revealed in its
/// place fails the test by 2.
#[test]
fn a_cancelled_token_contributes_nothing_and_a_new_token_is_revealed() {
    let session = willpower_test(
        board(
            &[CANCEL_AND_REDRAW],
            [ChaosToken::ElderSign, ChaosToken::Numeric(0)],
            FIRST_THEN_SECOND,
        ),
        5,
    );
    let session = pick_cancel_and_redraw(session);

    // Once per test: the new token's `when` cell offers the reaction no more.
    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::ElderSign,
            resolution: TokenResolution::ElderSign,
        },
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(0),
            ..
        },
        Event::SkillTestFailed { by: 2, .. },
    );
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
}

/// A Fight carrying a `[skull]` reveal clause. Cancelled, the skull fires no
/// clause; the `0` revealed in its place passes the attack.
#[test]
fn a_cancelled_token_fires_no_reveal_clause() {
    let mut ghoul: Enemy = test_support::test_enemy(GHOUL.0, "Ghoul");
    ghoul.fight = 3;
    ghoul.max_health = 3;
    ghoul.current_location = Some(HERE);
    ghoul.engaged_with = Some(INV);
    let mut state = board(
        &[CANCEL_AND_REDRAW, SKULL_CLAUSE_FIGHT],
        [ChaosToken::Skull, ChaosToken::Numeric(0)],
        FIRST_THEN_SECOND,
    );
    state.enemies.insert(GHOUL, ghoul);
    let fight = TurnAction::ActivateAbility {
        investigator: INV,
        source: AbilitySource::InPlay(state.in_play(INV, SKULL_CLAUSE_FIGHT)),
        address: AbilityAddress::Printed(0),
    };

    let session = TestSession::new(state, registries())
        .take(&fight)
        .commit(&[]);
    let session = pick_cancel_and_redraw(session);

    session.assert_at_turn_menu(INV);
    assert_event_sequence!(
        session.events(),
        Event::ChaosTokenRevealed {
            token: ChaosToken::Skull,
            ..
        },
        Event::ChaosTokenRevealed {
            token: ChaosToken::Numeric(0),
            ..
        },
        Event::SkillTestSucceeded { .. },
    );
    assert_no_event!(session.events(), Event::ResourcesGained { .. });
}

/// The cancelled token goes back in the bag — it never left it — so a
/// one-token bag redraws the same elder sign, which then resolves: its `+5`
/// passes the test and its symbol effect gains one resource, once.
#[test]
fn the_redraw_can_draw_the_same_token() {
    let session = willpower_test(board(&[CANCEL_AND_REDRAW], [ChaosToken::ElderSign], 0), 5);
    let session = pick_cancel_and_redraw(session);

    session.assert_at_turn_menu(INV);
    assert_event_count!(
        session.events(),
        2,
        Event::ChaosTokenRevealed {
            token: ChaosToken::ElderSign,
            ..
        }
    );
    assert_event_count!(session.events(), 1, Event::ResourcesGained { .. });
    assert_event!(
        session.events(),
        Event::SkillTestSucceeded { margin: 3, .. }
    );
}

/// *"Limit once per test"*: used in one test, the reaction is offered again in
/// the next.
#[test]
fn a_skill_test_usage_limit_resets_for_the_next_test() {
    let session = willpower_test(board(&[CANCEL_AND_REDRAW], [ChaosToken::Numeric(0)], 0), 3);
    let session = pick_cancel_and_redraw(session);
    session.assert_at_turn_menu(INV);

    let session = session.test_skill(INV, SkillKind::Willpower, 3).commit(&[]);
    assert_eq!(
        session.offered_at(&cancel_and_redraw(&session)),
        1,
        "the next test's reveal offers the reaction again"
    );
    let session = pick_cancel_and_redraw(session);
    session.assert_at_turn_menu(INV);
}

/// A cancel with nothing revealing a new token leaves the test at ST.5 with no
/// token: a malformed card, caught in debug builds.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "chaos token cancelled and no new token revealed")]
fn a_cancel_without_a_redraw_is_a_debug_assertion() {
    let session = willpower_test(board(&[CANCEL_ONLY], [ChaosToken::Numeric(0)], 0), 3);
    let target = session.state().card(INV, CANCEL_ONLY);
    let _ = session.pick(target);
}

/// A result known before step 3 skips the reveal, so no token is announced and
/// no `when` cell opens for the reaction: the test runs straight back to the
/// turn menu once its player windows are declined.
#[test]
fn a_determination_before_the_reveal_opens_no_when_cell() {
    let state = board(
        &[CANCEL_AND_REDRAW, AUTO_SUCCEED],
        [ChaosToken::Numeric(0)],
        0,
    );
    let auto_succeed = state.card(INV, AUTO_SUCCEED);
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, 9)
        .pick(auto_succeed)
        .skip()
        .commit(&[])
        .skip();

    session.assert_at_turn_menu(INV);
    assert_no_event!(session.events(), Event::ChaosTokenRevealed { .. });
    assert_event!(session.events(), Event::SkillTestSucceeded { .. });
}
