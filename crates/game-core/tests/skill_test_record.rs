//! What the in-flight skill test records for display: the revealed chaos
//! token with its resolution (`InFlightSkillTest::revealed`), and the ST.5
//! total the ST.6 comparison used (`ResolvedTest::total`).
//!
//! The web client still reconstructs both from the store's event latches
//! (`crates/web/src/skill_test_result.rs`). These fields let it read them off
//! state. They are records only: the engine never
//! reads either back (ADR 0005, ADR 0007).
//!
//! The frame is torn down at ST.8, so each test runs with
//! `interactive_acknowledge` on. That pauses the session at the post-ST.6
//! acknowledge, where the frame is readable through `current_skill_test`.
//!
//! Its own integration-test binary, so its `MockRegistry` and mock scenario
//! registry get a process to themselves.

use std::sync::LazyLock;

use card_dsl::dsl::{self, Cost, Determination, IntExpr, Quantity, TokenOutcome};
use game_core::engine::{OptionTarget, Registries};
use game_core::event::Event;
use game_core::scenario::{
    ScenarioEnding, ScenarioId, ScenarioModule, ScenarioRegistry, SymbolCtx,
};
use game_core::state::{
    ChaosBag, ChaosToken, GameState, GameStateBuilder, InvestigatorId, LocationId, RevealedToken,
    SkillKind, TokenResolution,
};
use game_core::test_support::{self, seat, MockRegistry, SeatLookup as _, TestSession, TEST_INV};

const SCENARIO: &str = "skill-test-record-mock";
const INV: InvestigatorId = InvestigatorId(1);
const HERE: LocationId = LocationId(10);

/// Mock asset: `[fast] Spend 1 resource: this skill test is automatically
/// successful.` Activated at ST.1, it latches the determination before the
/// draw, so ST.3 is skipped.
const AUTO_SUCCEED: &str = "MOCK-AS";

/// The mock symbol table: a `[skull]` is `+1 for each clue on your location`,
/// a live modifier, so the announced number is the one evaluated at reveal.
fn mock_resolve_symbol(token: ChaosToken, _ctx: &SymbolCtx) -> TokenOutcome {
    match token {
        ChaosToken::Skull => {
            TokenOutcome::modifier(IntExpr::Count(Quantity::CluesAtControllerLocation))
        }
        _ => TokenOutcome::default(),
    }
}

fn mock_setup() -> GameState {
    GameStateBuilder::new().build()
}

fn mock_apply_resolution(_: ScenarioEnding, _: &mut GameState, _: &mut Vec<Event>) {}

static MODULE: ScenarioModule = ScenarioModule {
    resolve_symbol: Some(mock_resolve_symbol),
    setup: mock_setup,
    apply_resolution: mock_apply_resolution,
    layout: &[],
};

fn module_for(id: &ScenarioId) -> Option<&'static ScenarioModule> {
    (id.as_str() == SCENARIO).then_some(&MODULE)
}

fn registries() -> Registries {
    static REGISTRIES: LazyLock<Registries> = LazyLock::new(|| Registries {
        cards: MockRegistry::new()
            // The fixture investigator's elder sign: a flat +2. A synthetic
            // sign (ADR 0016) models the primitive, not a printed card.
            .with_abilities(TEST_INV, || vec![dsl::elder_sign(IntExpr::Lit(2))])
            .with_abilities(AUTO_SUCCEED, || {
                vec![dsl::activated(
                    0,
                    vec![Cost::Resources(1)],
                    dsl::auto_resolve(Determination::AutomaticSuccess),
                )]
            })
            .build(),
        scenarios: &ScenarioRegistry { module_for },
    });
    *REGISTRIES
}

/// A willpower-3 investigator at `HERE` (holding two clues), with an in-play
/// instance of each code in `in_play`, drawing only `token`, on their open
/// turn, with the post-ST.6 acknowledge on so the test pauses there.
fn board(token: ChaosToken, in_play: &[&str]) -> GameState {
    let mut here = test_support::test_location(HERE.0, "Here");
    here.clues = 2;
    let mut state = GameStateBuilder::new()
        .with_registries(registries())
        .with_seat(
            seat(1)
                .skill(SkillKind::Willpower, 3)
                .at(HERE)
                .in_play(in_play.iter().copied()),
        )
        .with_location(here)
        .with_chaos_bag(ChaosBag::new([token]))
        .with_scenario_id(ScenarioId::new(SCENARIO))
        .open_turn(INV)
        .build();
    state.interactive_acknowledge = true;
    state
}

/// Run a difficulty-1 willpower test drawing `token`, committing nothing, to
/// the post-ST.6 pause. Returns the `revealed` record and the `total`.
fn paused_after(token: ChaosToken) -> (Option<RevealedToken>, i8) {
    let session = TestSession::new(board(token, &[]), registries())
        .test_skill(INV, SkillKind::Willpower, 1)
        .commit(&[]);
    read_record(&session)
}

fn read_record(session: &TestSession) -> (Option<RevealedToken>, i8) {
    let test = session
        .state()
        .current_skill_test()
        .expect("the acknowledge pause holds the test frame open");
    let resolved = test.resolved.expect("the pause follows ST.6");
    (test.revealed, resolved.total)
}

/// The record of drawing `token` with nothing substituting it: it counts as
/// itself.
fn drawn(token: ChaosToken, resolution: TokenResolution) -> RevealedToken {
    RevealedToken {
        token,
        counts_as: token,
        resolution,
    }
}

#[test]
fn a_numeric_token_is_recorded_and_counts_in_the_total() {
    // Willpower 3, -1: total 2.
    let (revealed, total) = paused_after(ChaosToken::Numeric(-1));
    assert_eq!(
        revealed,
        Some(drawn(
            ChaosToken::Numeric(-1),
            TokenResolution::Modifier(-1)
        ))
    );
    assert_eq!(total, 2);
}

#[test]
fn a_scenario_symbol_is_recorded_at_its_value_on_reveal() {
    // Two clues at the location: the skull is +2, so willpower 3 totals 5.
    let (revealed, total) = paused_after(ChaosToken::Skull);
    assert_eq!(
        revealed,
        Some(drawn(ChaosToken::Skull, TokenResolution::Modifier(2)))
    );
    assert_eq!(total, 5);
}

#[test]
fn an_elder_sign_is_recorded_and_its_modifier_counts_in_the_total() {
    // The investigator's elder sign is +2, so willpower 3 totals 5.
    let (revealed, total) = paused_after(ChaosToken::ElderSign);
    assert_eq!(
        revealed,
        Some(drawn(ChaosToken::ElderSign, TokenResolution::ElderSign))
    );
    assert_eq!(total, 5);
}

/// A determination latched before ST.3 skips the draw (*"No chaos token(s)
/// are revealed from the chaos bag"*, `glossary/Automatic_Failure_Success.md`),
/// so nothing is recorded as revealed. The total is still the real ST.5
/// skill value: only the difficulty is substituted to 0.
#[test]
fn a_skipped_draw_records_no_token() {
    let state = board(ChaosToken::Numeric(-1), &[AUTO_SUCCEED]);
    let card = state.in_play(INV, AUTO_SUCCEED);
    let session = TestSession::new(state, registries())
        .test_skill(INV, SkillKind::Willpower, 9)
        .pick(OptionTarget::CardInstance(card))
        .skip()
        .commit(&[])
        .skip();
    let (revealed, total) = read_record(&session);
    assert_eq!(revealed, None);
    assert_eq!(total, 3);
}
